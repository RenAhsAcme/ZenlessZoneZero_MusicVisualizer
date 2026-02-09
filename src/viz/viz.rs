//! WGPU可视化渲染模块
//!
//! 该模块使用WGPU图形API实现音频频谱的实时可视化渲染
//! 采用winit创建窗口，实现64个频段的柱状图显示效果
//!
//! 主要特性：
//! - 实时频谱柱状图渲染
//! - 平滑动画效果
//! - 响应式窗口大小调整
//! - 中心水平线装饰效果
// 导入必要的crate和模块
use crate::dsp::spectrum::{BANDS, SharedPipe}; // 频谱数据相关
use pollster::block_on; // 异步运行时阻塞执行
use std::mem::size_of; // 内存大小计算
// WGPU图形API相关导入
use wgpu::{
    BlendState, Color, ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompositeAlphaMode,
    DeviceDescriptor, FragmentState, Instance, MultisampleState, PipelineLayoutDescriptor,
    PowerPreference, PresentMode, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, StoreOp,
    SurfaceConfiguration, TextureUsages, VertexBufferLayout, VertexState, VertexStepMode,
    util::{BufferInitDescriptor, DeviceExt},
    vertex_attr_array,
};
// Winit窗口系统相关导入
use winit::{
    application::ApplicationHandler,              // 应用程序事件处理器
    event::WindowEvent,                           // 窗口事件类型
    event_loop::{ActiveEventLoop, EventLoop},     // 事件循环
    window::{Window, WindowAttributes, WindowId}, // 窗口相关类型
};
/// 顶点数据结构
///
/// 表示2D图形的顶点位置信息
/// 使用repr(C)确保内存布局与着色器匹配
/// 实现Pod和Zeroable trait用于高效缓冲区操作
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2], // 2D坐标位置 [x, y]
}
impl Vertex {
    /// 获取顶点缓冲区布局描述
    ///
    /// 定义顶点数据在内存中的组织方式
    fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: size_of::<Vertex>() as _, // 每个顶点的字节大小
            step_mode: VertexStepMode::Vertex,      // 顶点步进模式
            attributes: &vertex_attr_array![0 => Float32x2], // 位置属性：2个f32
        }
    }
}
/// 启动可视化渲染
///
/// 初始化WGPU渲染环境并启动主渲染循环
///
/// # 参数
/// * `shared` - 频谱数据共享管道
pub fn run(shared: SharedPipe) {
    // 使用pollster阻塞执行异步代码
    block_on(async move {
        // 初始化频谱平滑数据
        // let mut smooth_bands = vec![0.0f32; BANDS];  // 平滑后的频段数据
        // const SMOOTHING: f32 = 0.2;                  // 平滑系数
        // let raw = shared.read();                     // 读取初始频谱数据
        //
        // // 应用初始平滑处理
        // for i in 0..BANDS {
        //     smooth_bands[i] = smooth_bands[i] * (1.0 - SMOOTHING) + raw[i] * SMOOTHING;
        // }
        /// 应用程序主结构体
        ///
        /// 包含所有渲染相关的状态和资源
        struct App {
            window: Option<Window>,                 // 窗口对象
            instance: Option<Instance>,             // WGPU实例
            adapter: Option<wgpu::Adapter>,         // GPU适配器
            device: Option<wgpu::Device>,           // 逻辑设备
            queue: Option<wgpu::Queue>,             // 命令队列
            pipeline: Option<wgpu::RenderPipeline>, // 渲染管线
            config: Option<SurfaceConfiguration>,   // 表面配置
            t: f32,                                 // 时间计数器
            smooth_bands: Vec<f32>,                 // 平滑频段数据
            shared: SharedPipe,                     // 频谱数据管道
            vertex_buffer: Option<wgpu::Buffer>,    // 顶点缓冲区
            max_vertices: usize,                    // 最大顶点数
        }
        impl ApplicationHandler for App {
            /// 应用恢复时的回调
            ///
            /// 初始化窗口和基本渲染资源
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                // 创建主窗口
                let attrs = WindowAttributes::default().with_title("Explore Demo");
                let window = event_loop.create_window(attrs).unwrap();
                self.window = Some(window);

                // 初始化WGPU实例
                self.instance = Some(Instance::default());
                self.t = 0.0; // 重置时间计数器
                self.max_vertices = BANDS * 6; // 预估最大顶点数
            }
            /// 处理窗口事件
            ///
            /// 响应各种窗口事件，主要是重绘请求
            fn window_event(
                &mut self,
                event_loop: &ActiveEventLoop,
                _window_id: WindowId,
                event: WindowEvent,
            ) {
                match event {
                    WindowEvent::CloseRequested => {
                        // 用户请求关闭窗口
                        event_loop.exit();
                    }
                    WindowEvent::RedrawRequested => {
                        if let (Some(window), Some(instance)) = (&self.window, &self.instance) {
                            let surface = instance.create_surface(window).unwrap();
                            // 首次渲染时初始化GPU资源
                            if self.device.is_none() {
                                // 请求合适的GPU适配器
                                if let Ok(adapter) =
                                    block_on(instance.request_adapter(&RequestAdapterOptions {
                                        power_preference: PowerPreference::LowPower, // 低功耗优先
                                        compatible_surface: Some(&surface),          // 兼容表面
                                        force_fallback_adapter: false,               // 不强制回退
                                    }))
                                {
                                    // 请求逻辑设备和命令队列
                                    if let Ok((device, queue)) = block_on(
                                        adapter.request_device(&DeviceDescriptor::default()),
                                    ) {
                                        // 获取表面支持的纹理格式
                                        let format = surface.get_capabilities(&adapter).formats[0];

                                        // 配置表面参数
                                        let config = SurfaceConfiguration {
                                            usage: TextureUsages::RENDER_ATTACHMENT, // 用作渲染目标
                                            format,                                  // 纹理格式
                                            width: window.inner_size().width,        // 宽度
                                            height: window.inner_size().height,      // 高度
                                            present_mode: PresentMode::Fifo,         // FIFO呈现模式
                                            desired_maximum_frame_latency: 1,        // 最大帧延迟
                                            alpha_mode: CompositeAlphaMode::Auto, // 自动alpha模式
                                            view_formats: vec![],                 // 视图格式
                                        };
                                        // 应用表面配置
                                        surface.configure(&device, &config);

                                        // 创建着色器模块
                                        let shader =
                                            device.create_shader_module(ShaderModuleDescriptor {
                                                label: None,
                                                source: ShaderSource::Wgsl(
                                                    include_str!("shader.wgsl").into(), // 包含WGSL着色器代码
                                                ),
                                            });
                                        // 创建管线布局（着色器资源绑定配置）
                                        let pipeline_layout = device.create_pipeline_layout(
                                            &PipelineLayoutDescriptor {
                                                label: None,
                                                bind_group_layouts: &[], // 无需绑定组
                                                push_constant_ranges: &[], // 无需push常量
                                            },
                                        );
                                        // 创建渲染管线
                                        let pipeline = device.create_render_pipeline(
                                            &RenderPipelineDescriptor {
                                                label: None,
                                                layout: Some(&pipeline_layout), // 使用创建的管线布局
                                                vertex: VertexState {
                                                    module: &shader,              // 顶点着色器模块
                                                    entry_point: Some("vs_main"), // 顶点着色器入口点
                                                    compilation_options: Default::default(),
                                                    buffers: &[Vertex::desc()], // 顶点缓冲区布局
                                                },
                                                fragment: Some(FragmentState {
                                                    module: &shader,              // 片段着色器模块
                                                    entry_point: Some("fs_main"), // 片段着色器入口点
                                                    compilation_options: Default::default(),
                                                    targets: &[Some(ColorTargetState {
                                                        format,                                  // 渲染目标格式
                                                        blend: Some(BlendState::ALPHA_BLENDING), // Alpha混合
                                                        write_mask: ColorWrites::ALL, // 写入所有颜色通道
                                                    })],
                                                }),
                                                primitive: PrimitiveState::default(), // 默认图元装配
                                                depth_stencil: None, // 无需深度模板
                                                multisample: MultisampleState::default(), // 默认多重采样
                                                multiview: None, // 无需多视图
                                                cache: None,     // 无需缓存
                                            },
                                        );
                                        // 存储初始化好的GPU资源
                                        self.adapter = Some(adapter); // GPU适配器句柄
                                        self.device = Some(device); // 逻辑设备句柄
                                        self.queue = Some(queue); // 命令队列句柄
                                        self.pipeline = Some(pipeline); // 渲染管线句柄
                                        self.config = Some(config); // 表面配置
                                    }
                                }
                            }
                            // 确保所有必需的渲染资源都已初始化完成
                            // 这是执行实际渲染的前提条件
                            if let (Some(device), Some(queue), Some(pipeline)) =
                                (&self.device, &self.queue, &self.pipeline)
                            {
                                // 处理窗口尺寸变化的响应式渲染
                                if let Some(config) = &self.config {
                                    let current_width = window.inner_size().width; // 当前窗口宽度
                                    let current_height = window.inner_size().height; // 当前窗口高度

                                    // 检测窗口尺寸是否发生改变
                                    if config.width != current_width
                                        || config.height != current_height
                                    {
                                        // 窗口大小已改变，需要更新表面配置
                                        let mut new_config = config.clone();
                                        new_config.width = current_width; // 更新宽度
                                        new_config.height = current_height; // 更新高度
                                        surface.configure(device, &new_config); // 重新配置表面
                                        self.config = Some(new_config); // 保存新配置
                                    } else {
                                        // 窗口大小未变，使用现有配置
                                        surface.configure(device, config);
                                    }
                                }
                                // 获取当前帧的渲染目标纹理
                                let output = surface.get_current_texture().unwrap();

                                // 为纹理创建视图，用于后续的渲染操作
                                let view = output
                                    .texture
                                    .create_view(&wgpu::TextureViewDescriptor::default());

                                // 创建命令编码器，用于记录GPU命令
                                let mut encoder = device
                                    .create_command_encoder(&CommandEncoderDescriptor::default());
                                // 预分配顶点容器以提高性能
                                let mut vertices = Vec::with_capacity(self.max_vertices);
                                let bars = 64; // 要显示的频谱柱数量
                                let raw = self.shared.read(); // 从共享管道读取最新的频谱数据
                                const SMOOTHING: f32 = 0.03; // 频谱数据平滑系数

                                // 为每个频段生成对应的可视化柱状图
                                for i in 0..BANDS.min(bars) {
                                    // 根据频段位置应用不同的平滑系数
                                    // 低频段使用更强的平滑效果以减少抖动
                                    let freq_smooth = if i < BANDS / 6 {
                                        SMOOTHING // * 3.0 // 低频段三倍平滑强度
                                    } else {
                                        SMOOTHING // 其他频段正常使用平滑
                                    };

                                    // 应用指数移动平均滤波器进行数据平滑
                                    // 公式：y[n] = α×x[n] + (1-α)×y[n-1]
                                    self.smooth_bands[i] = self.smooth_bands[i]
                                        * (1.0 - freq_smooth)
                                        + raw[i] * freq_smooth;
                                    // 计算当前柱状图的水平位置坐标
                                    let x0 = -1.0 + 2.0 * i as f32 / bars as f32; // 左边界 [-1.0, 1.0]
                                    let x1 = x0 + 2.0 / bars as f32 * 0.8; // 右边界（占80%宽度）

                                    // 处理频谱值并应用非线性变换增强视觉效果
                                    let v = self.smooth_bands[i].clamp(0.0, 1.0); // 限制值域到[0,1]
                                    let h = (v * 3.0).tanh(); // 双曲正切函数增强对比度
                                    let half = h * 0.5; // 柱状图高度的一半
                                    // 定义柱状图四个关键点的垂直坐标
                                    let y_top_0 = 0.0; // 上方柱状图底部（Y=0）
                                    let y_top_1 = half; // 上方柱状图顶部
                                    let y_bot_0 = 0.0; // 下方柱状图顶部（Y=0）
                                    let y_bot_1 = -half; // 下方柱状图底部
                                    // 中心水平装饰线的几何参数
                                    let line_thickness = 0.01; // 装饰线的垂直厚度
                                    let line_left = -1.0; // 线条左端点（屏幕左边界）
                                    let line_right = 1.0; // 线条右端点（屏幕右边界）
                                    vertices.extend_from_slice(&[
                                        Vertex {
                                            position: [x0, y_top_0],
                                        },
                                        Vertex {
                                            position: [x1, y_top_0],
                                        },
                                        Vertex {
                                            position: [x1, y_top_1],
                                        },
                                        Vertex {
                                            position: [x0, y_top_0],
                                        },
                                        Vertex {
                                            position: [x1, y_top_1],
                                        },
                                        Vertex {
                                            position: [x0, y_top_1],
                                        },
                                        Vertex {
                                            position: [x0, y_bot_0],
                                        },
                                        Vertex {
                                            position: [x1, y_bot_0],
                                        },
                                        Vertex {
                                            position: [x1, y_bot_1],
                                        },
                                        Vertex {
                                            position: [x0, y_bot_0],
                                        },
                                        Vertex {
                                            position: [x1, y_bot_1],
                                        },
                                        Vertex {
                                            position: [x0, y_bot_1],
                                        },
                                        Vertex {
                                            position: [line_left, -line_thickness],
                                        },
                                        Vertex {
                                            position: [line_right, -line_thickness],
                                        },
                                        Vertex {
                                            position: [line_right, line_thickness],
                                        },
                                        Vertex {
                                            position: [line_left, -line_thickness],
                                        },
                                        Vertex {
                                            position: [line_right, line_thickness],
                                        },
                                        Vertex {
                                            position: [line_left, line_thickness],
                                        },
                                    ]);
                                }
                                // 更新或创建顶点缓冲区
                                let vertex_buffer =
                                    if let Some(ref existing_buffer) = self.vertex_buffer {
                                        // 如果已有缓冲区，使用暂存缓冲区进行更新
                                        let staging_buffer =
                                            device.create_buffer_init(&BufferInitDescriptor {
                                                label: Some("顶点数据暂存缓冲区"),
                                                contents: bytemuck::cast_slice(&vertices),
                                                usage: wgpu::BufferUsages::COPY_SRC, // 用作复制源
                                            });

                                        // 创建命令编码器执行缓冲区复制
                                        let mut encoder = device.create_command_encoder(
                                            &CommandEncoderDescriptor { label: None },
                                        );

                                        // 执行缓冲区数据复制
                                        encoder.copy_buffer_to_buffer(
                                            &staging_buffer,
                                            0,
                                            existing_buffer,
                                            0,
                                            (vertices.len() * size_of::<Vertex>()) as u64,
                                        );

                                        // 提交复制命令
                                        queue.submit(Some(encoder.finish()));
                                        existing_buffer
                                    } else {
                                        // 首次创建顶点缓冲区
                                        let buffer =
                                            device.create_buffer_init(&BufferInitDescriptor {
                                                label: Some("频谱柱顶点缓冲区"),
                                                contents: bytemuck::cast_slice(&vertices),
                                                usage: wgpu::BufferUsages::VERTEX      // 顶点缓冲区用途
                                                    | wgpu::BufferUsages::COPY_DST, // 可接受复制目标
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
                                                view: &view,          // 渲染目标视图
                                                depth_slice: None,    // 无需深度切片
                                                resolve_target: None, // 无需解析目标
                                                ops: wgpu::Operations {
                                                    load: wgpu::LoadOp::Clear(Color::BLACK), // 清屏为黑色
                                                    store: StoreOp::Store, // 存储渲染结果
                                                },
                                            })],
                                            depth_stencil_attachment: None, // 无需深度模板附件
                                            timestamp_writes: None,         // 无需时间戳
                                            occlusion_query_set: None,      // 无需遮挡查询
                                        });

                                    // 设置渲染管线和顶点缓冲区
                                    rpass.set_pipeline(pipeline); // 应用渲染管线
                                    rpass.set_vertex_buffer(0, vertex_buffer.slice(..)); // 绑定顶点缓冲区

                                    // 执行绘制命令
                                    rpass.draw(0..vertices.len() as u32, 0..1); // 绘制所有顶点
                                }
                                // 提交渲染命令并呈现结果
                                queue.submit(Some(encoder.finish())); // 提交命令队列
                                output.present(); // 呈现当前帧

                                // 请求下一帧重绘
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
            smooth_bands: vec![0.0f32; BANDS],
            shared,
            vertex_buffer: None,
            max_vertices: BANDS * 6,
        };
        let _ = event_loop.run_app(&mut app);
    });
}
