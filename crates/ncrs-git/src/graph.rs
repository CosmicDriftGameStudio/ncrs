use gix::ObjectId;

/// Which part of a row's height a line covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentHalf {
    /// From the top of the row to the commit's dot: `from` is the lane the line
    /// comes in on, `to` the lane of the dot.
    Upper,
    /// From the dot to the bottom of the row: `from` is the lane of the dot, `to`
    /// the lane of the parent the line goes on in. A fork or merge curve when
    /// the two differ.
    Lower,
    /// A lane that passes the row without touching the dot; `from` equals `to`.
    Through,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphSegment {
    pub half: SegmentHalf,
    pub from: usize,
    pub to: usize,
}

/// What the graph column draws in the row of one commit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    pub lane: usize,
    pub segments: Vec<GraphSegment>,
}

/// Assigns lanes to commits handed in newest first, children before parents.
#[derive(Debug, Default)]
pub struct LaneLayout {
    expected: Vec<Option<ObjectId>>,
    lane_count: usize,
}

impl LaneLayout {
    pub(crate) fn lane_count(&self) -> usize {
        self.lane_count
    }

    pub(crate) fn place(&mut self, id: ObjectId, parents: &[ObjectId]) -> GraphRow {
        let incoming: Vec<usize> = self
            .expected
            .iter()
            .enumerate()
            .filter(|(_, slot)| **slot == Some(id))
            .map(|(lane, _)| lane)
            .collect();
        let lane = incoming
            .first()
            .copied()
            .unwrap_or_else(|| self.free_lane());

        let mut segments = Vec::new();
        for (other, slot) in self.expected.iter().enumerate() {
            if slot.is_some() && !incoming.contains(&other) {
                segments.push(GraphSegment {
                    half: SegmentHalf::Through,
                    from: other,
                    to: other,
                });
            }
        }
        for &from in &incoming {
            segments.push(GraphSegment {
                half: SegmentHalf::Upper,
                from,
                to: lane,
            });
            self.set(from, None);
        }
        self.set(lane, None);

        for (position, parent) in parents.iter().enumerate() {
            let known = self.expected.iter().position(|slot| *slot == Some(*parent));
            let to = match known {
                Some(existing) => existing,
                None if position == 0 => lane,
                None => self.free_lane(),
            };
            self.set(to, Some(*parent));
            segments.push(GraphSegment {
                half: SegmentHalf::Lower,
                from: lane,
                to,
            });
        }
        self.lane_count = self.lane_count.max(self.expected.len()).max(lane + 1);
        GraphRow { lane, segments }
    }

    fn free_lane(&mut self) -> usize {
        match self.expected.iter().position(Option::is_none) {
            Some(lane) => lane,
            None => {
                self.expected.push(None);
                self.expected.len() - 1
            }
        }
    }

    fn set(&mut self, lane: usize, value: Option<ObjectId>) {
        while self.expected.len() <= lane {
            self.expected.push(None);
        }
        if let Some(slot) = self.expected.get_mut(lane) {
            *slot = value;
        }
    }
}
