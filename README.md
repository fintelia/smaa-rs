# smaa-rs [![crates.io](https://img.shields.io/crates/v/smaa.svg)](https://crates.io/crates/smaa) [![docs.rs](https://docs.rs/smaa/badge.svg)](https://docs.rs/smaa)

Post-process antialiasing for wgpu-rs, relying on the [SMAA reference implementation](https://github.com/iryoku/smaa).

# Example

```rust
// Create SMAA target
let mut smaa_target = SmaaTarget::new(
    &device,
    &queue,
    window.inner_size().width,
    window.inner_size().height,
    wgpu::TextureFormat::Rgba8Unorm,
    swapchain_format
)?;

// Main loop
event_loop.run(move |event, _, control_flow| {
    match event {
        Event::RedrawRequested(_) => {
            let frame = smaa_target.color_target();

            // Render the scene into `frame`.
            // [...]
   
            let output_frame = swap_chain.get_current_frame().unwrap().output;
            smaa_target.resolve(&device, &queue, &output_frame.view);
        }
        _ => {}
    }
});

```
