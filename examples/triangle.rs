use smaa::*;
use std::borrow::Cow;
use std::sync::Arc;
use wgpu::{ColorTargetState, ColorWrites};
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;

fn main() {
    // Initialize wgpu
    let event_loop: EventLoop<()> = EventLoop::new().unwrap();
    let window = winit::window::Window::new(&event_loop).unwrap();
    let window_size = window.inner_size();
    let window_arc = Arc::new(window);
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let surface = instance.create_surface(window_arc.clone()).unwrap();
    let adapter =
        futures::executor::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        futures::executor::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let swapchain_format = surface.get_capabilities(&adapter).formats[0];
    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: swapchain_format,
        width: window_size.width,
        height: window_size.height,
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    // Create SMAA target
    let mut smaa_target = SmaaTarget::new(
        &device,
        &queue,
        window_size.width,
        window_size.height,
        swapchain_format,
        SmaaMode::Smaa1X,
    );

    // Prepare scene
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[],
        push_constant_ranges: &[],
    });
    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format: swapchain_format,
                blend: None,
                write_mask: ColorWrites::all(),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None
    });

    // Main loop
    let _ = event_loop.run(move |event, event_loop| {
        if let Event::WindowEvent { event, .. } = event {
            match event {
                WindowEvent::Resized(size) => {
                    // Recreate the swap chain with the new size
                    config.width = size.width;
                    config.height = size.height;
                    surface.configure(&device, &config);
                    smaa_target.resize(&device, size.width, size.height);
                }
                WindowEvent::RedrawRequested => {
                    let output_frame = surface.get_current_texture().unwrap();
                    let output_view = output_frame.texture.create_view(&Default::default());
                    let smaa_frame = smaa_target.start_frame(&device, &queue, &output_view);

                    let mut encoder = device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                    {
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: None,
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &smaa_frame,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            occlusion_query_set: None,
                            timestamp_writes: None,
                        });
                        rpass.set_pipeline(&render_pipeline);
                        rpass.draw(0..3, 0..1);
                    }
                    queue.submit(Some(encoder.finish()));

                    smaa_frame.resolve();
                    output_frame.present();
                }
                WindowEvent::CloseRequested => event_loop.exit(),
                _ => (),
            }
        }
    });
}
