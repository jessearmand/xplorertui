use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;
use ratatui_image::Image;
use ratatui_image::picker::ProtocolType;

use crate::app::PhotoView;
use crate::app::photos::{FocusedPhoto, PHOTO_ROWS};

/// The focused post's photo in the thread view, with a caption line.
pub struct PhotoPanel<'a> {
    view: Option<&'a PhotoView>,
    focused: &'a FocusedPhoto,
    protocol: ProtocolType,
}

impl<'a> PhotoPanel<'a> {
    pub fn new(
        view: Option<&'a PhotoView>,
        focused: &'a FocusedPhoto,
        protocol: ProtocolType,
    ) -> Self {
        Self {
            view,
            focused,
            protocol,
        }
    }

    /// Rows the panel needs: the photo (or a status line) plus the caption.
    pub fn height(&self) -> u16 {
        let body = match self.view {
            Some(PhotoView::Ready(p)) => p.0.size().height.min(PHOTO_ROWS),
            _ => 1,
        };
        body + 1
    }

    fn caption(&self) -> String {
        let protocol = match self.protocol {
            ProtocolType::Halfblocks => "half blocks",
            ProtocolType::Sixel => "Sixel",
            ProtocolType::Kitty => "Kitty",
            ProtocolType::Iterm2 => "iTerm2",
        };
        let position = format!("photo {}/{}", self.focused.index + 1, self.focused.count);
        if self.focused.count > 1 {
            format!("{position} · [ ] next photo · {protocol}")
        } else {
            format!("{position} · {protocol}")
        }
    }
}

impl Widget for PhotoPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let dim = Style::default().fg(Color::DarkGray);
        let body = Rect {
            height: area.height.saturating_sub(1),
            ..area
        };
        match self.view {
            Some(PhotoView::Ready(protocol)) => {
                if let Some(placeholder) = protocol.0.needs_placeholder(body) {
                    buf.set_string(
                        placeholder.x,
                        placeholder.y,
                        "[photo: the window is too small]",
                        dim,
                    );
                } else {
                    Image::new(&protocol.0)
                        .allow_clipping(true)
                        .render(body, buf);
                }
            }
            Some(PhotoView::Failed(e)) => {
                let text: String = format!("[photo not loaded: {e}]")
                    .chars()
                    .take(area.width as usize)
                    .collect();
                buf.set_string(body.x, body.y, text, Style::default().fg(Color::Red));
            }
            Some(PhotoView::Loading) | None => {
                buf.set_string(body.x, body.y, "Loading photo…", dim);
            }
        }
        let caption_y = area.y + area.height - 1;
        let caption: String = self.caption().chars().take(area.width as usize).collect();
        buf.set_string(area.x, caption_y, caption, dim);
    }
}
