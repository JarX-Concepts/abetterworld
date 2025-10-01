use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
#[cfg(target_arch = "wasm32")]
use web_time::{Duration, Instant};

use crate::{
    content::{tiles, Tile, TileManager},
    helpers::{channel::Receiver, AbwError},
};

pub fn import_renderables(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    content: &Arc<TileManager>,
    receiver: &mut Receiver<Tile>,
    budget: Duration,
) -> Result<bool, AbwError> {
    log::info!("Importing renderables with budget {:?}", budget);
    if budget.is_zero() {
        return Ok(false);
    }

    let start = Instant::now();
    let mut needs_update = false;

    // Pull tiles until either the channel is empty or the time budget is spent.
    loop {
        // Respect the budget before starting another expensive setup.
        if start.elapsed() >= budget {
            break;
        }

        match receiver.try_recv() {
            Ok(mut tile) => {
                // If setup itself can be costly, double-check budget again.
                if start.elapsed() >= budget {
                    break;
                }

                let new_tile = tiles::content_render_setup(device, queue, layout, &mut tile)?;
                content.add_renderable(new_tile);
                needs_update = true;
            }
            Err(_) => break, // channel empty
        }
    }

    log::info!(
        "Importing renderables done, took {:?}, needs_update={}",
        start.elapsed(),
        needs_update
    );
    Ok(needs_update)
}
