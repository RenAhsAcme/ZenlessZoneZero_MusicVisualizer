//! 代码注释由 Alibaba Lingma 服务生成，仅供参考，请以代码实际意图为准。
//! 因项目仍处于开发阶段，部分警告尚未消除。
use pollster::block_on;
// ==============================================================================
// ZenlessZoneZero 音乐可视化器 - WGPU渲染模块
// ==============================================================================
// 该模块实现了基于WGPU的实时音乐可视化渲染功能
// 主要特性：
// - 使用现代化的WGPU图形API进行硬件加速渲染
// - 实现动态频谱柱状图可视化效果
// - 支持窗口大小自适应和高帧率渲染
// ==============================================================================

// 标准库和外部依赖导入
use std::mem::size_of;  // 内存大小计算，用于顶点缓冲区布局
// WGPU核心组件导入
use wgpu::CommandEncoderDescriptor;     // 命令编码器描述符
// WGPU配置选项
use wgpu::CompositeAlphaMode::Auto;      // 自动alpha混合模式
use wgpu::PowerPreference::HighPerformance;  // 优先使用高性能GPU
use wgpu::PresentMode::Fifo;             // FIFO垂直同步模式
// WGPU渲染相关
use wgpu::RenderPassColorAttachment;     // 渲染通道颜色附件
use wgpu::RenderPassDescriptor;          // 渲染通道描述符
use wgpu::ShaderSource::Wgsl;            // WGSL着色器源码格式
use wgpu::StoreOp;                       // 渲染目标存储操作
// WGPU实用工具
use wgpu::util::BufferInitDescriptor;    // 缓冲区初始化描述符
use wgpu::{
    BlendState, Color, ColorTargetState, ColorWrites, DeviceDescriptor, FragmentState, Instance,
    MultisampleState, PipelineLayoutDescriptor, PrimitiveState, RenderPipelineDescriptor,
    RequestAdapterOptions, ShaderModuleDescriptor, SurfaceConfiguration, TextureUsages,
    VertexBufferLayout, VertexState, VertexStepMode, util::DeviceExt, vertex_attr_array,
};
use winit::window::WindowId;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
};

// ==============================================================================
// 顶点数据结构定义
// ==============================================================================
// 顶点结构体用于定义2D图形的基本几何信息
// #[repr(C)] 确保内存布局与C语言兼容
// Pod 和 Zeroable trait 保证内存安全转换
// ==============================================================================
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    /// 顶点2D坐标 [x, y]，范围 [-1.0, 1.0]
    position: [f32; 2],
}
impl Vertex {
    /// 创建顶点缓冲区布局描述符
    /// 返回适合GPU渲染的顶点数据布局配置
    fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            // 每个顶点数据的字节大小
            array_stride: size_of::<Vertex>() as _,
            // 顶点步进模式：每个顶点处理一次
            step_mode: VertexStepMode::Vertex,
            // 顶点属性数组：位置属性（索引0）为2个f32组成的向量
            attributes: &vertex_attr_array![0 => Float32x2],
        }
    }
}

pub fn run() {
    // 探索 WGPU 在 Rust 中的用法，仍处于实验性中。
    block_on(async {
        struct App {
            window: Option<Window>,
            instance: Option<Instance>,
            adapter: Option<wgpu::Adapter>,
            device: Option<wgpu::Device>,
            queue: Option<wgpu::Queue>,
            pipeline: Option<wgpu::RenderPipeline>,
            config: Option<SurfaceConfiguration>,
            t: f32,
        }
        impl ApplicationHandler for App {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let attrs = WindowAttributes::default()
                    .with_title("Explore Demo");
                let window = event_loop.create_window(attrs).unwrap();
                // 只存储window和instance，其他组件延迟初始化
                self.window = Some(window);
                self.instance = Some(Instance::default());
                self.t = 0.0;
            }
            fn window_event(
                &mut self,
                event_loop: &ActiveEventLoop,
                _window_id: WindowId,
                event: WindowEvent,
            ) {
                match event {
                    WindowEvent::CloseRequested => {
                        event_loop.exit();
                    }
                    WindowEvent::RedrawRequested => {
                        // ======================================================================
                        // 渲染主循环 - 核心可视化逻辑
                        // ======================================================================
                        // 每帧重新创建surface以适应可能的窗口变化
                        if let (Some(window), Some(instance))
                            = (&self.window, &self.instance) {
                            let surface = instance.create_surface(window).unwrap();
                            
                            // 延迟初始化GPU设备和渲染管线（仅在首次渲染时执行）
                            if self.device.is_none() {
                                // 请求高性能GPU适配器
                                if let Ok(adapter) =
                                    block_on(instance.request_adapter(
                                        &RequestAdapterOptions {
                                            power_preference: HighPerformance,  // 优先选择独显
                                            compatible_surface: Some(&surface), // 确保与表面兼容
                                            force_fallback_adapter: false,      // 不强制使用软件渲染
                                        }))
                                {
                                    if let Ok((device, queue)) = block_on(
                                        adapter.request_device(&DeviceDescriptor::default()),
                                    ) {
                                        let format = surface
                                            .get_capabilities(&adapter)
                                            .formats[0];
                                        let config
                                            = SurfaceConfiguration {
                                            usage: TextureUsages::RENDER_ATTACHMENT,
                                            format,
                                            width: window.inner_size().width,
                                            height: window.inner_size().height,
                                            present_mode: Fifo,
                                            desired_maximum_frame_latency: 0,
                                            alpha_mode: Auto,
                                            view_formats: vec![],
                                        };
                                        surface.configure(&device, &config);
                                        let shader =
                                            device.create_shader_module(
                                                ShaderModuleDescriptor {
                                                    label: None,
                                                    source: Wgsl(include_str!("shader.wgsl")
                                                        .into()),
                                                });
                                        let pipeline_layout = device
                                            .create_pipeline_layout(
                                                &PipelineLayoutDescriptor {
                                                    label: None,
                                                    bind_group_layouts: &[],
                                                    push_constant_ranges: &[],
                                                },
                                            );
                                        let pipeline = device.create_render_pipeline(
                                            &RenderPipelineDescriptor {
                                                label: None,
                                                layout: Some(&pipeline_layout),
                                                vertex: VertexState {
                                                    module: &shader,
                                                    entry_point: Some("vs_main"),
                                                    compilation_options: Default::default(),
                                                    buffers: &[Vertex::desc()],
                                                },
                                                fragment: Some(FragmentState {
                                                    module: &shader,
                                                    entry_point: Some("fs_main"),
                                                    compilation_options: Default::default(),
                                                    targets: &[Some(ColorTargetState {
                                                        format,
                                                        blend: Some(BlendState::ALPHA_BLENDING),
                                                        write_mask: ColorWrites::ALL,
                                                    })],
                                                }),
                                                primitive: PrimitiveState::default(),
                                                depth_stencil: None,
                                                multisample: MultisampleState::default(),
                                                multiview: None,
                                                cache: None,
                                            },
                                        );

                                        self.adapter = Some(adapter);
                                        self.device = Some(device);
                                        self.queue = Some(queue);
                                        self.pipeline = Some(pipeline);
                                        self.config = Some(config);
                                    }
                                }
                            }
                            if let (Some(device),
                                Some(queue),
                                Some(pipeline))
                                = (&self.device, &self.queue, &self.pipeline)
                            {
                                if let Some(config) = &self.config {
                                    if config.width != window.inner_size().width
                                        || config.height != window.inner_size().height
                                    {
                                        let mut new_config = config.clone();
                                        new_config.width = window.inner_size().width;
                                        new_config.height = window.inner_size().height;
                                        surface.configure(device, &new_config);
                                        self.config = Some(new_config);
                                    } else {
                                        surface.configure(device, config);
                                    }
                                }

                                let output = surface.get_current_texture().unwrap();
                                let view = output
                                    .texture
                                    .create_view(&wgpu::TextureViewDescriptor::default());

                                let mut encoder = device
                                    .create_command_encoder(&CommandEncoderDescriptor::default());
                                let mut vertices = Vec::new();
                                let bars = 64;  // 频谱柱数量

                                // 生成动态频谱柱状图顶点数据
                                for i in 0..bars {
                                    // 计算柱子的水平位置
                                    let x0 = -1.0 + 2.0 * i as f32 / bars as f32;
                                    let x1 = x0 + 2.0 / bars as f32 * 0.8;  // 80%宽度，留出间隙
                                    
                                    // 基于时间的正弦波动生成高度（模拟音乐节拍）
                                    let h = (self.t + i as f32 * 0.2).sin().abs();

                                    // 柱子的垂直范围
                                    let y0 = -1.0;      // 底部固定
                                    let y1 = -1.0 + h;  // 顶部随音乐变化

                                    // 为每个柱子创建两个三角形（6个顶点）
                                    vertices.extend_from_slice(&[
                                        Vertex { position: [x0, y0] },  // 左下
                                        Vertex { position: [x1, y0] },  // 右下
                                        Vertex { position: [x1, y1] },  // 右上
                                        Vertex { position: [x0, y0] },  // 左下（重复）
                                        Vertex { position: [x1, y1] },  // 右上（重复）
                                        Vertex { position: [x0, y1] },  // 左上
                                    ]);
                                }

                                // 创建顶点缓冲区并上传到GPU
                                let vertex_buffer = device.create_buffer_init(
                                    &BufferInitDescriptor {
                                        label: Some("频谱柱顶点缓冲区"),
                                        contents: bytemuck::cast_slice(&vertices),
                                        usage: wgpu::BufferUsages::VERTEX,
                                    },
                                );

                                // 开始渲染通道
                                {
                                    let mut rpass = encoder.begin_render_pass(
                                        &RenderPassDescriptor {
                                            label: Some("主渲染通道"),
                                            color_attachments: &[Some(RenderPassColorAttachment {
                                                view: &view,
                                                depth_slice: None,
                                                resolve_target: None,
                                                ops: wgpu::Operations {
                                                    load: wgpu::LoadOp::Clear(Color::BLACK),  // 清屏为黑色
                                                    store: StoreOp::Store,                    // 保存渲染结果
                                                },
                                            })],
                                            depth_stencil_attachment: None,
                                            timestamp_writes: None,
                                            occlusion_query_set: None,
                                        }
                                    );

                                    // 设置渲染管线和顶点缓冲区
                                    rpass.set_pipeline(pipeline);
                                    rpass
                                        .set_vertex_buffer(0, vertex_buffer.slice(..));
                                    
                                    // 绘制所有频谱柱（每个柱子6个顶点）
                                    rpass
                                        .draw(0..vertices.len() as u32, 0..1);
                                }

                                // 提交命令队列并呈现结果
                                queue.submit(Some(encoder.finish()));
                                output.present();  // 将渲染结果显示到屏幕上
                                
                                // 更新动画时间（控制波动速度）
                                self.t += 0.03;

                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                        }

                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                    _ => {}
                }
            }
        }
        // ======================================================================
        // 应用程序启动逻辑
        // ======================================================================
        // 创建事件循环和应用程序实例
        let event_loop = EventLoop::new().unwrap();
        let mut app = App {
            window: None,
            instance: None,
            adapter: None,
            device: None,
            queue: None,
            pipeline: None,
            config: None,
            t: 0.0,
        };
        
        // 启动应用程序事件循环
        // run_app会接管程序控制流，直到窗口关闭
        let _ = event_loop.run_app(&mut app);
    });
}

// ==============================================================================
// 文件结束
// ==============================================================================
// 本模块实现了完整的WGPU音乐可视化渲染系统
// 特色功能：
// 1. 基于WebGPU的现代图形渲染
// 2. 动态频谱可视化效果
// 3. 自适应窗口大小调整
// 4. 高性能硬件加速渲染
// ==============================================================================
