pub type MediaDb = sleigh::Tree<Media, Vec<Op>, sleigh::encoding::Bincode>;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
enum MediaVersioned {
    // Never remove variants from this enum, or the tag on the wire goes out of sync.
    // Never edit existing versions directly.
    // Instead:
    //
    // - edit `Media` fields
    // - add new `V(last+1)` variant, copy-paste `Media` fields into it
    // - edit `Media::serialize` changing `V(prev)` to the new `V(new)`
    // - add match arms and fix destructurings and struct instantiation until code compiles
    V1 { exists: bool },
}

#[derive(serde::Deserialize, Debug, Default)]
#[serde(from = "MediaVersioned")]
pub struct Media {
    // DO NOT EDIT directly, see MediaVersioned.
    /// Exists on disk to the best of our knowledge.
    pub exists: bool,
}

impl serde::Serialize for Media {
    // Prefer this way over `#[serde(into="MediaVersioned")])` because this way we don't need to implement `Clone` for `Media`.
    // This could probably be cleaned up with `serde(with)` for containers, if and when that is implemented: https://github.com/serde-rs/serde/issues/1118
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Change this to `V(latest)` when changing `Media`.
        let ver = MediaVersioned::V1 {
            exists: self.exists,
        };
        ver.serialize(serializer)
    }
}

impl From<MediaVersioned> for Media {
    fn from(ver: MediaVersioned) -> Self {
        match ver {
            MediaVersioned::V1 { exists } => Media { exists },
            // Add new `V(n)` variants here.
        }
    }
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
