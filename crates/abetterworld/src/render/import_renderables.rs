#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
use std::{ops::DerefMut, sync::Arc};
#[cfg(target_arch = "wasm32")]
use web_time::{Duration, Instant};

use crate::{
    content::{tiles, TilePipelineMessage},
    helpers::{channel::Receiver, AbwError},
    render::SceneGraph,
};

pub fn import_renderables(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    content: &mut SceneGraph,
    receiver: &mut Receiver<TilePipelineMessage>,
    budget: Duration,
) -> Result<bool, AbwError> {
    if budget.is_zero() {
        return Ok(false);
    }

    let start = Instant::now();
    let mut needs_update = false;

    // Pull tiles until either the channel is empty or the time budget is spent.
    loop {
        // Respect the budget before starting another expensive setup.
        if start.elapsed() >= budget {
            //break;
        }

        match receiver.try_recv() {
            Ok(tile_message) => {
                // If setup itself can be costly, double-check budget again.
                if start.elapsed() >= budget {
                    break;
                }

                match tile_message {
                    TilePipelineMessage::Load(message) => {
                        let new_tile =
                            tiles::content_render_setup(device, queue, layout, message.1)?;
                        content.add_renderable(message.0.key, new_tile);
                    }
                    TilePipelineMessage::Unload(message) => {
                        content.remove(message);
                    }
                    TilePipelineMessage::Update(message) => {
                        content.add_info(message);
                    }
                };

                needs_update = true;
            }
            Err(_) => break, // channel empty
        }
    }

    Ok(needs_update)
}
