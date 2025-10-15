use crate::{
    content::{TileSource, TileSourceContent, TileSourceContentState},
    dynamics::CameraRefinementData,
    helpers::{is_bounding_volume_visible, AbwError},
};
use cgmath::MetricSpace;

pub struct Pri<'a> {
    pub tile: &'a TileSource,
    pub tile_content: &'a TileSourceContent,
    pub priority: f64,
}

pub fn gather_priority_tiles<'a>(
    camera_data: &CameraRefinementData,
    tile: &'a TileSource,
    out_inview: &mut Vec<Pri<'a>>,
    out_outofview: &mut Vec<Pri<'a>>,
) -> Result<(), AbwError> {
    if let Some(needs_refinement) = tile.needs_refinement_flag {
        if let Some(content) = &tile.content {
            match &content.loaded {
                Some(TileSourceContentState::ToLoadVisual) => {
                    let distance = camera_data
                        .position
                        .distance2(tile.bounding_volume.center());
                    let priority_tile = Pri {
                        tile,
                        tile_content: content,
                        priority: distance,
                    };

                    if is_bounding_volume_visible(
                        &camera_data.planes,
                        &tile.bounding_volume.to_aabb(),
                    ) {
                        out_inview.push(priority_tile);
                    } else {
                        out_outofview.push(priority_tile);
                    }
                }
                Some(TileSourceContentState::LoadedTileSet { permanent, .. }) => {
                    if needs_refinement {
                        if let Some(permanent_root) = permanent.as_ref() {
                            if let Some(root) = &permanent_root.root {
                                gather_priority_tiles(
                                    camera_data,
                                    root,
                                    out_inview,
                                    out_outofview,
                                )?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if needs_refinement {
            if let Some(children) = &tile.children {
                for child in children {
                    gather_priority_tiles(camera_data, child, out_inview, out_outofview)?;
                }
            }
        }
    }

    Ok(())
}

pub fn priortize<'a>(
    camera_data: &CameraRefinementData,
    tile: &'a TileSource,
    out: &mut Vec<Pri<'a>>,
) -> Result<(), AbwError> {
    let mut inview: Vec<Pri> = Vec::new();
    let mut outofview: Vec<Pri> = Vec::new();
    gather_priority_tiles(camera_data, tile, &mut inview, &mut outofview)?;

    // sort by priority (distance)
    inview.sort_unstable_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap());
    outofview.sort_unstable_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap());

    out.extend(inview);
    out.extend(outofview);

    Ok(())
}
