//! `compare`: one picture with the web interface's screenshot of a moment on the left and Beryl
//! 3D's frame of the same moment on the right, so that a session sees both in one read. Beryl 3D
//! keeps the web interface's look (docs/beryl-3d.md, "Migration and compatibility"), and no
//! check compares whole pictures (decision C5), so a look is judged by reading the two side by
//! side. `just b3d compare` shoots the web page from the recording's exported fixture and passes
//! its path here as `--web`.

use std::fmt::Write;
use std::path::Path;

use beryl_inspect::cli::{self, Options, Usage};

use crate::{Error, file_name, open, out_dir};

/// The space between the two pictures, in pixels.
const GAP: u32 = 16;

/// The colour of that space and of the part of the shorter picture's side below it: a grey
/// that neither interface's table is.
const BETWEEN: [u8; 4] = [128, 128, 128, 255];

pub(crate) fn compare(options: &Options) -> Result<String, Error> {
    let web_path = options.value("--web").ok_or_else(|| {
        Usage(
            "compare needs --web <png>, the web interface's screenshot of the moment; \
             `just b3d compare` takes it for you"
                .to_owned(),
        )
    })?;
    let web = beryl_render::read_rgba_png(Path::new(web_path)).map_err(Error::Failed)?;
    let mut head = open(options)?;
    cli::seek_and_act(head.page(), options, &options.at)?;
    let (width, height) = head.size();
    let native = head.pixels();
    let (pixels, total_width, total_height) = beside(
        &web.pixels,
        (web.width, web.height),
        &native,
        (width, height),
    );
    let path = out_dir()?.join(file_name(options, &format!("{}-compare", options.at)));
    beryl_render::write_rgba_png(&path, &pixels, total_width, total_height)
        .map_err(Error::Failed)?;
    let mut out = format!(
        "{}\nwrote {} ({total_width}x{total_height} px)\n",
        options.command_line(),
        path.display()
    );
    let _ = writeln!(
        out,
        "left: the web interface ({}x{} px, {web_path})\nright: Beryl 3D ({width}x{height} px)\n{}",
        web.width,
        web.height,
        cli::position_line(head.page())
    );
    Ok(out)
}

/// `left` and `right`, RGBA pictures of the sizes given, side by side with `GAP` between them
/// and their tops level, and the size of the whole.
pub(crate) fn beside(
    left: &[u8],
    (left_width, left_height): (u32, u32),
    right: &[u8],
    (right_width, right_height): (u32, u32),
) -> (Vec<u8>, u32, u32) {
    let width = left_width + GAP + right_width;
    let height = left_height.max(right_height);
    let mut pixels = BETWEEN.repeat((width * height) as usize);
    for (source, source_width, source_height, x) in [
        (left, left_width, left_height, 0),
        (right, right_width, right_height, left_width + GAP),
    ] {
        let row_bytes = (source_width * 4) as usize;
        for row in 0..source_height {
            let from = row as usize * row_bytes;
            let to = ((row * width + x) * 4) as usize;
            pixels[to..to + row_bytes].copy_from_slice(&source[from..from + row_bytes]);
        }
    }
    (pixels, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pictures_stand_side_by_side_with_their_tops_level() {
        let red = [255, 0, 0, 255].repeat(2 * 3);
        let blue = [0, 0, 255, 255].repeat(4);
        let (pixels, width, height) = beside(&red, (2, 3), &blue, (1, 4));
        assert_eq!((width, height), (2 + GAP + 1, 4));
        let at = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
        };
        assert_eq!(at(1, 2), [255, 0, 0, 255]);
        assert_eq!(at(0, 3), BETWEEN, "below the shorter picture");
        assert_eq!(at(2, 0), BETWEEN, "the gap");
        assert_eq!(at(2 + GAP, 3), [0, 0, 255, 255]);
    }
}
