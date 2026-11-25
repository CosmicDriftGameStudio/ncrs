//! The graph column of a commit list: one cell per commit with round lanes, a
//! colour per lane and a ring instead of a dot for a merge.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineCap, Path, Stroke};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Theme};

use crate::palette::Palette;
use crate::theme::{git, size};

/// Horizontal room of one lane.
pub const LANE_WIDTH: f32 = 14.0;
const LINE_WIDTH: f32 = 2.0;
const DOT_RADIUS: f32 = 4.0;
const DIMMED_ALPHA: f32 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineHalf {
    /// From the top of the row to the dot of the commit.
    Upper,
    /// From the dot to the bottom of the row.
    Lower,
    /// Past the dot, from the top to the bottom.
    Through,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphLine {
    pub half: LineHalf,
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCell {
    pub lane: usize,
    pub lines: Vec<GraphLine>,
    /// Drawn as a ring.
    pub merge: bool,
    pub dimmed: bool,
}

/// The colour of a lane; the lanes take turns in cyan, pink and orange. Yellow
/// is left to keyboard focus.
pub fn lane_color(palette: &Palette, lane: usize) -> Color {
    let colors = [git::cyan(), palette.tagged, git::orange()];
    colors
        .get(lane % colors.len())
        .copied()
        .unwrap_or(palette.text)
}

#[derive(Debug)]
struct GraphProgram {
    cell: GraphCell,
    palette: Palette,
}

impl GraphProgram {
    fn color(&self, lane: usize) -> Color {
        let color = lane_color(&self.palette, lane);
        if self.cell.dimmed {
            Color {
                a: color.a * DIMMED_ALPHA,
                ..color
            }
        } else {
            color
        }
    }
}

pub(crate) fn lane_x(lane: usize) -> f32 {
    LANE_WIDTH * lane as f32 + LANE_WIDTH / 2.0
}

impl<M> canvas::Program<M> for GraphProgram {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let height = bounds.height;
        let middle = height / 2.0;
        for line in &self.cell.lines {
            let (path, lane) = match line.half {
                LineHalf::Through => (
                    Path::line(
                        Point::new(lane_x(line.from), 0.0),
                        Point::new(lane_x(line.from), height),
                    ),
                    line.from,
                ),
                LineHalf::Upper => (
                    Path::new(|builder| {
                        builder.move_to(Point::new(lane_x(line.from), 0.0));
                        builder.quadratic_curve_to(
                            Point::new(lane_x(line.from), middle),
                            Point::new(lane_x(line.to), middle),
                        );
                    }),
                    line.from,
                ),
                LineHalf::Lower => (
                    Path::new(|builder| {
                        builder.move_to(Point::new(lane_x(line.from), middle));
                        builder.quadratic_curve_to(
                            Point::new(lane_x(line.to), middle),
                            Point::new(lane_x(line.to), height),
                        );
                    }),
                    line.to,
                ),
            };
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(self.color(lane))
                    .with_width(LINE_WIDTH)
                    .with_line_cap(LineCap::Round),
            );
        }
        let dot = Path::circle(Point::new(lane_x(self.cell.lane), middle), DOT_RADIUS);
        let color = self.color(self.cell.lane);
        if self.cell.merge {
            frame.fill(&dot, self.palette.panel);
            frame.stroke(
                &dot,
                Stroke::default().with_color(color).with_width(LINE_WIDTH),
            );
        } else {
            frame.fill(&dot, color);
        }
        vec![frame.into_geometry()]
    }
}

/// The graph of one commit, `lanes` wide so every row of the list lines up.
pub fn graph_cell<'a, M: 'a>(cell: GraphCell, lanes: usize, palette: &Palette) -> Element<'a, M> {
    Canvas::new(GraphProgram {
        cell,
        palette: *palette,
    })
    .width(Length::Fixed(LANE_WIDTH * lanes.max(1) as f32))
    .height(Length::Fixed(size::ROW))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_take_turns_in_colours() {
        let palette = Palette::default();
        assert_eq!(lane_color(&palette, 0), lane_color(&palette, 3));
    }

    #[test]
    fn no_lane_is_drawn_in_the_focus_yellow() {
        let palette = Palette::default();
        for lane in 0..6 {
            assert_ne!(lane_color(&palette, lane), palette.accent);
        }
    }
}
