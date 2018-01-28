# gfx_smaa [![crates.io](https://img.shields.io/crates/v/gfx_smaa.svg)](https://crates.io/crates/gfx_smaa) [![docs.rs](https://docs.rs/gfx_smaa/badge.svg)](https://docs.rs/gfx_smaa) [![Travis](https://img.shields.io/travis/fintelia/gfx_smaa.svg)]()

A library for post process antialiasing for the gfx-rs graphics API, based on the [SMAA
reference implementation](https://github.com/iryoku/smaa). Currently only works with OpenGL 3+, but support for other graphics APIs is planned.

# Example

```rust
// create window
let mut window: PistonWindow = WindowSettings::new("SMAA", (640, 480)).build().unwrap();

// create target
let mut target = SmaaTarget::new(&mut window.factory,
                                 window.output_color.clone(),
                                 640, 480).unwrap();

// main loop
while let Some(e) = window.next() {
    window.draw_3d(&e, |window| {
        // clear depth and color buffers.
        window.encoder.clear_depth(&target.output_stencil(), 1.0);
        window.encoder.clear(&target.output_color(), [0.0, 0.0, 0.0, 1.0]);

        // Render the scene.
        ...

        // Perform actual antialiasing operation and write the result to the screen.
        target.resolve(&mut window.encoder);
     });
}
```
