pub type MediaDb = sleigh::Tree<Media, Vec<Op>, sleigh::encoding::Bincode>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Media {
    /// Exists on disk to the best of our knowledge.
    pub exists: bool,
}

impl sleigh::Merge<Vec<Op>> for Media {
    fn merge(&mut self, ops: Vec<Op>) -> sleigh::MergeVerdict {
        for op in ops {
            match op {
                Op::Exists(b) => self.exists = b,
            }
        }
        sleigh::MergeVerdict::Keep
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub enum Op {
    // Never remove variants from this enum, or the tag on the wire goes out of sync.
    Exists(bool),
}
