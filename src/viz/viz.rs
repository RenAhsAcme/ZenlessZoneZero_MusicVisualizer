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
use std::mem::size_of; // 内存大小计算，用于顶点缓冲区布局
// WGPU核心组件导入
use wgpu::CommandEncoderDescriptor; // 命令编码器描述符
// WGPU配置选项
use wgpu::CompositeAlphaMode::Auto; // 自动alpha混合模式
use wgpu::PowerPreference::HighPerformance; // 优先使用高性能GPU
use wgpu::PresentMode::{Fifo, Mailbox}; // FIFO垂直同步模式
// WGPU渲染相关
use wgpu::RenderPassColorAttachment; // 渲染通道颜色附件
use wgpu::RenderPassDescriptor; // 渲染通道描述符
use wgpu::ShaderSource::Wgsl; // WGSL着色器源码格式
use wgpu::StoreOp; // 渲染目标存储操作
// WGPU实用工具
use crate::dsp::spectrum::{BANDS, SharedPipe};
use wgpu::util::BufferInitDescriptor; // 缓冲区初始化描述符
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

pub fn run(shared: SharedPipe) {
    // 探索 WGPU 在 Rust 中的用法，仍处于实验性中。

    block_on(async move {
        // 构造平滑器？
        let mut smooth_bands = vec![0.0f32; BANDS];
        const SMOOTHING: f32 = 0.2;
        let raw = shared.read();
        for i in 0..BANDS {
            smooth_bands[i] = smooth_bands[i] * (1.0 - SMOOTHING) + raw[i] * SMOOTHING;
        }
        struct App {
            window: Option<Window>,
            instance: Option<Instance>,
            adapter: Option<wgpu::Adapter>,
            device: Option<wgpu::Device>,
            queue: Option<wgpu::Queue>,
            pipeline: Option<wgpu::RenderPipeline>,
            config: Option<SurfaceConfiguration>,
            t: f32,
            smooth_bands: Vec<f32>, // 添加平滑频谱数据
            shared: SharedPipe,     // 添加共享管道引用
            // 预创建的顶点缓冲区以提高性能
            vertex_buffer: Option<wgpu::Buffer>,
            max_vertices: usize, // 最大顶点数
        }
        impl ApplicationHandler for App {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                let attrs = WindowAttributes::default().with_title("Explore Demo");
                let window = event_loop.create_window(attrs).unwrap();

                // 只存储window和instance，其他组件延迟初始化
                self.window = Some(window);
                self.instance = Some(Instance::default());
                self.t = 0.0;
                // 平滑频谱数据已经在构造时初始化
                // 预分配顶点缓冲区内存
                self.max_vertices = BANDS * 6; // 每个频段6个顶点
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
                        if let (Some(window), Some(instance)) = (&self.window, &self.instance) {
                            let surface = instance.create_surface(window).unwrap();

                            // 延迟初始化GPU设备和渲染管线（仅在首次渲染时执行）
                            if self.device.is_none() {
                                // 请求高性能GPU适配器
                                if let Ok(adapter) =
                                    block_on(instance.request_adapter(&RequestAdapterOptions {
                                        power_preference: HighPerformance,  // 优先选择独显
                                        compatible_surface: Some(&surface), // 确保与表面兼容
                                        force_fallback_adapter: false,      // 不强制使用软件渲染
                                    }))
                                {
                                    if let Ok((device, queue)) = block_on(
                                        adapter.request_device(&DeviceDescriptor::default()),
                                    ) {
                                        let format = surface.get_capabilities(&adapter).formats[0];
                                        let config = SurfaceConfiguration {
                                            usage: TextureUsages::RENDER_ATTACHMENT,
                                            format,
                                            width: window.inner_size().width,
                                            height: window.inner_size().height,
                                            present_mode: Fifo,
                                            desired_maximum_frame_latency: 1, // 降低延迟
                                            alpha_mode: Auto,
                                            view_formats: vec![],
                                        };
                                        surface.configure(&device, &config);
                                        let shader =
                                            device.create_shader_module(ShaderModuleDescriptor {
                                                label: None,
                                                source: Wgsl(include_str!("shader.wgsl").into()),
                                            });
                                        let pipeline_layout = device.create_pipeline_layout(
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
                            if let (Some(device), Some(queue), Some(pipeline)) =
                                (&self.device, &self.queue, &self.pipeline)
                            {
                                // 配置surface（每次都重新配置以处理窗口大小变化）
                                if let Some(config) = &self.config {
                                    let current_width = window.inner_size().width;
                                    let current_height = window.inner_size().height;

                                    if config.width != current_width
                                        || config.height != current_height
                                    {
                                        // 窗口大小发生变化，需要重新配置
                                        let mut new_config = config.clone();
                                        new_config.width = current_width;
                                        new_config.height = current_height;
                                        surface.configure(device, &new_config);
                                        self.config = Some(new_config);
                                    } else {
                                        // 窗口大小未变，正常使用现有配置
                                        surface.configure(device, config);
                                    }
                                }

                                let output = surface.get_current_texture().unwrap();
                                let view = output
                                    .texture
                                    .create_view(&wgpu::TextureViewDescriptor::default());

                                let mut encoder = device
                                    .create_command_encoder(&CommandEncoderDescriptor::default());
                                let mut vertices = Vec::with_capacity(self.max_vertices);
                                let bars = 64; // 频谱柱数量

                                // 生成动态频谱柱状图顶点数据
                                const SMOOTHING: f32 = 0.03; // 极致降低平滑度，实现毫秒级响应
                                let raw = self.shared.read(); // 从共享管道读取原始数据

                                for i in 0..BANDS.min(bars) {
                                    // 确保不会越界
                                    // 更新平滑频谱数据
                                    let freq_smooth = if i < BANDS / 6 {
                                        // 仅最低频段保留轻微平滑避免严重抖动
                                        SMOOTHING * 3.0
                                    } else {
                                        // 其他所有频段几乎无平滑
                                        SMOOTHING
                                    };

                                    self.smooth_bands[i] = self.smooth_bands[i]
                                        * (1.0 - freq_smooth)
                                        + raw[i] * freq_smooth;

                                    // 计算柱子的水平位置
                                    let x0 = -1.0 + 2.0 * i as f32 / bars as f32;
                                    let x1 = x0 + 2.0 / bars as f32 * 0.8; // 80%宽度，留出间隙

                                    // 使用平滑后的频谱数据生成高度（提高灵敏度）
                                    let v = self.smooth_bands[i].clamp(0.0, 1.0);
                                    let h = v * 0.9; // 增加高度比例提高视觉冲击力

                                    // 柱子的垂直范围
                                    let y0 = -1.0; // 底部固定
                                    let y1 = -1.0 + h; // 顶部随音乐变化

                                    // 为每个柱子创建两个三角形（6个顶点）
                                    vertices.extend_from_slice(&[
                                        Vertex { position: [x0, y0] }, // 左下
                                        Vertex { position: [x1, y0] }, // 右下
                                        Vertex { position: [x1, y1] }, // 右上
                                        Vertex { position: [x0, y0] }, // 左下（重复）
                                        Vertex { position: [x1, y1] }, // 右上（重复）
                                        Vertex { position: [x0, y1] }, // 左上
                                    ]);
                                }

                                // 重用或创建顶点缓冲区
                                let vertex_buffer = if let Some(ref existing_buffer) =
                                    self.vertex_buffer
                                {
                                    // 更新现有缓冲区内容
                                    let staging_buffer =
                                        device.create_buffer_init(&BufferInitDescriptor {
                                            label: Some("顶点数据暂存缓冲区"),
                                            contents: bytemuck::cast_slice(&vertices),
                                            usage: wgpu::BufferUsages::COPY_SRC,
                                        });

                                    let mut encoder = device.create_command_encoder(
                                        &wgpu::CommandEncoderDescriptor { label: None },
                                    );
                                    encoder.copy_buffer_to_buffer(
                                        &staging_buffer,
                                        0,
                                        existing_buffer,
                                        0,
                                        (vertices.len() * std::mem::size_of::<Vertex>()) as u64,
                                    );
                                    queue.submit(Some(encoder.finish()));

                                    existing_buffer
                                } else {
                                    // 首次创建缓冲区
                                    let buffer = device.create_buffer_init(&BufferInitDescriptor {
                                        label: Some("频谱柱顶点缓冲区"),
                                        contents: bytemuck::cast_slice(&vertices),
                                        usage: wgpu::BufferUsages::VERTEX
                                            | wgpu::BufferUsages::COPY_DST,
                                    });
                                    self.vertex_buffer = Some(buffer);
                                    self.vertex_buffer.as_ref().unwrap()
                                };

                                // 开始渲染通道
                                {
                                    let mut rpass =
                                        encoder.begin_render_pass(&RenderPassDescriptor {
                                            label: Some("主渲染通道"),
                                            color_attachments: &[Some(RenderPassColorAttachment {
                                                view: &view,
                                                depth_slice: None,
                                                resolve_target: None,
                                                ops: wgpu::Operations {
                                                    load: wgpu::LoadOp::Clear(Color::BLACK), // 清屏为黑色
                                                    store: StoreOp::Store, // 保存渲染结果
                                                },
                                            })],
                                            depth_stencil_attachment: None,
                                            timestamp_writes: None,
                                            occlusion_query_set: None,
                                        });

                                    // 设置渲染管线和顶点缓冲区
                                    rpass.set_pipeline(pipeline);
                                    rpass.set_vertex_buffer(0, vertex_buffer.slice(..));

                                    // 绘制所有频谱柱（每个柱子6个顶点）
                                    rpass.draw(0..vertices.len() as u32, 0..1);
                                }

                                // 提交命令队列并呈现结果
                                queue.submit(Some(encoder.finish()));
                                output.present(); // 将渲染结果显示到屏幕上

                                // 更新动画时间（控制波动速度）
                                // self.t += 0.01;

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
            smooth_bands: vec![0.0f32; BANDS], // 初始化平滑频谱数据
            shared,                            // 传递共享管道
            vertex_buffer: None,
            max_vertices: BANDS * 6, // 预分配顶点缓冲区大小
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
