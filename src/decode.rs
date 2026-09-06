use std::io::{self, BufRead, Cursor, Read, Seek, SeekFrom};

use image::{AnimationDecoder, DynamicImage, ImageDecoder};

use crate::config::ImageLimits;
use crate::display::AnimFrame;
use crate::shutdown::Work;

/// Check cancellation at decoder reads as well as between animation frames.
struct CheckedReader<'a, 'b> {
    inner: Cursor<&'a [u8]>,
    work: &'b Work<'b>,
}

impl CheckedReader<'_, '_> {
    fn check(&self) -> io::Result<()> {
        self.work.check().map_err(io::Error::other)
    }
}

impl Read for CheckedReader<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        self.inner.read(buf)
    }
}

impl BufRead for CheckedReader<'_, '_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.check()?;
        // Bound the amount available before the decoder has to check again.
        let buf = self.inner.fill_buf()?;
        Ok(&buf[..buf.len().min(8192)])
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

impl Seek for CheckedReader<'_, '_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.check()?;
        self.inner.seek(pos)
    }
}

pub fn image(data: &[u8], limits: &ImageLimits, work: &Work<'_>) -> anyhow::Result<DynamicImage> {
    work.check()?;
    let input = CheckedReader {
        inner: Cursor::new(data),
        work,
    };
    let mut reader = image::ImageReader::new(input).with_guessed_format()?;
    reader.limits(limits.decoder_limits());
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    limits.check_dimensions(width, height)?;
    anyhow::ensure!(
        decoder.total_bytes() <= limits.max_decoded_bytes,
        "decoded image exceeds MAX_DECODED_BYTES"
    );
    work.check()?;
    let image = DynamicImage::from_decoder(decoder)?;
    work.check()?;
    Ok(image)
}

pub fn gif(data: &[u8], limits: &ImageLimits, work: &Work<'_>) -> anyhow::Result<Vec<AnimFrame>> {
    work.check()?;
    let input = CheckedReader {
        inner: Cursor::new(data),
        work,
    };
    let mut decoder = image::codecs::gif::GifDecoder::new(input)?;
    let (width, height) = decoder.dimensions();
    limits.check_dimensions(width, height)?;
    // Split the budget between decoder scratch space and retained RGBA frames.
    // image's GIF iterator accounts for its canvas and temporary frame buffers.
    let scratch = limits.max_decoded_bytes / 2;
    let mut decoder_limits = limits.decoder_limits();
    decoder_limits.max_alloc = Some(scratch);
    decoder.set_limits(decoder_limits)?;
    let frame_bytes = u64::from(width) * u64::from(height) * 4;
    anyhow::ensure!(
        frame_bytes <= limits.max_decoded_bytes - scratch,
        "GIF frame exceeds MAX_DECODED_BYTES"
    );
    let mut frames = Vec::new();
    let mut retained = 0u64;
    let mut source = decoder.into_frames();
    loop {
        work.check()?;
        let Some(frame) = source.next() else { break };
        let frame = frame?;
        work.check()?;
        anyhow::ensure!(
            frames.len() < limits.max_frames,
            "GIF exceeds MAX_ANIMATION_FRAMES ({})",
            limits.max_frames
        );
        retained = retained
            .checked_add(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("GIF decoded size overflow"))?;
        anyhow::ensure!(
            retained <= limits.max_decoded_bytes - scratch,
            "GIF frames exceed MAX_DECODED_BYTES"
        );
        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = if num == 0 || den == 0 {
            100
        } else {
            u64::from(num).div_ceil(u64::from(den)).clamp(10, 60_000)
        };
        frames.push(AnimFrame {
            image: DynamicImage::ImageRgba8(frame.into_buffer()),
            delay: std::time::Duration::from_millis(delay_ms),
        });
    }
    anyhow::ensure!(!frames.is_empty(), "GIF has no frames");
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{shutdown::Shutdown, test_support};
    use std::time::{Duration, Instant};

    #[test]
    fn rejects_oversized_dimensions_before_decoding_pixels() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let limits = ImageLimits {
            max_dimension: 4,
            ..ImageLimits::default()
        };
        assert!(image(&test_support::png(5, 1), &limits, &work).is_err());
        assert!(gif(&test_support::gif(5, 1, 1), &limits, &work).is_err());
    }

    #[test]
    fn animation_frame_limit_accepts_boundary_and_rejects_next_frame() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let limits = ImageLimits {
            max_frames: 2,
            ..ImageLimits::default()
        };
        assert_eq!(
            gif(&test_support::gif(2, 2, 2), &limits, &work)
                .unwrap()
                .len(),
            2
        );
        let err = gif(&test_support::gif(2, 2, 3), &limits, &work)
            .err()
            .unwrap();
        assert!(err.to_string().contains("MAX_ANIMATION_FRAMES"));
    }

    #[test]
    fn animation_budget_counts_retained_frames() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        // 128 bytes for scratch, 128 for eight retained 2x2 RGBA frames.
        let limits = ImageLimits {
            max_decoded_bytes: 256,
            ..ImageLimits::default()
        };
        assert_eq!(
            gif(&test_support::gif(2, 2, 8), &limits, &work)
                .unwrap()
                .len(),
            8
        );
        assert!(gif(&test_support::gif(2, 2, 9), &limits, &work).is_err());
    }

    #[test]
    fn static_output_budget_is_enforced() {
        let shutdown = Shutdown::new();
        let work = Work {
            shutdown: &shutdown,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let limits = ImageLimits {
            max_decoded_bytes: 128,
            ..ImageLimits::default()
        };
        assert!(image(&test_support::png(16, 16), &limits, &work).is_err());
    }
}
