#[allow(unused_imports)]
use async_std::prelude::*;
use async_std::sync::{Arc, Condvar, Mutex};
use choosy_protocol as proto;
use im::{ordmap, OrdMap};
use std::collections::VecDeque;

#[derive(Clone, PartialEq)]
struct File {}

pub struct List {
    updated: Condvar,

    // TODO maybe migrate away from im
    //
    // https://docs.rs/immutable-chunkmap/0.5.9/immutable_chunkmap/map/struct.Map.html
    // https://docs.rs/rpds/0.8.0/rpds/map/red_black_tree_map/struct.RedBlackTreeMap.html
    //
    // but none of the alternatives have cheap diff (immutable_chunkmap diff method only lets you reason about the intersection, fixes key type so can't produce an Add/Del enum, and doesn't promise cheapness)
    //
    // reasons:
    // https://github.com/bodil/im-rs/issues/152
    // https://github.com/bodil/im-rs/issues/153
    files: Arc<Mutex<OrdMap<String, File>>>,
}

impl List {
    pub fn new() -> Self {
        Self {
            updated: Condvar::new(),
            files: Arc::new(Mutex::new(OrdMap::new())),
        }
    }

    pub async fn update(&self, changes: impl Iterator<Item = proto::FileChange>) {
        let mut files = self.files.lock().await;
        let prev = files.clone();
        for change in changes {
            match change {
                proto::FileChange::ClearAll => {
                    files.clear();
                }
                proto::FileChange::Add { name } => {
                    files.insert(name.to_string(), File {});
                }
                proto::FileChange::Del { name } => {
                    files.remove(&name);
                }
            };
        }
        if *files != prev {
            self.updated.notify_all();
        }
    }

    pub fn change_batches(&self) -> ChangeBatches {
        ChangeBatches {
            list: self,
            prev: ordmap! {},
        }
    }
}

pub struct ChangeBatches<'a> {
    list: &'a List,
    prev: OrdMap<String, File>,
}

impl<'a> ChangeBatches<'a> {
    // This looks superficially like async_std::stream::Stream but isn't one, because the async ecosystem teamed up with the borrow checker to bully me.
    //
    // Since this is always never-ending, I've removed Option from the API too.

    pub async fn next(&mut self) -> ChangeIterator {
        let files = {
            let mut lock = self.list.files.lock().await;
            // wait for file list to change
            while *lock == self.prev {
                lock = self.list.updated.wait(lock).await;
            }
            lock.clone()
        };
        let diffs = self
            .prev
            .diff(&files)
            .filter_map(|diff| match diff {
                ordmap::DiffItem::Add(name, _file) => Some(proto::FileChange::Add {
                    name: name.to_string(),
                }),
                ordmap::DiffItem::Update {
                    old: (_old_name, _old_file),
                    new: (_new_name, _new_file),
                } => {
                    // TODO send file value info to client
                    None
                }
                ordmap::DiffItem::Remove(name, _file) => Some(proto::FileChange::Del {
                    name: name.to_string(),
                }),
            })
            // I've spent too much time fighting the borrow checker here. Doing `prev.diff(&files)` here leaves the diff forever borrowing `files`, and cloning didn't help because then it just borrowed that temporary value! And returning it in the iterator makes it live past this function call. And even ordmap::DiffIter holds references. Punt and collect in-memory; the whole list is in memory anyway.
            .collect::<VecDeque<proto::FileChange>>();

        self.prev = files;

        ChangeIterator {
            iter: Arc::new(std::sync::Mutex::new(diffs)),
        }
    }
}

pub struct ChangeIterator {
    // This struct exists purely so we can slap an Arc+Mutex in between, so we can use this as Stream Item.
    //
    // Mutex is sync not async because it'll be consumed in Iterator, which is not async.
    iter: Arc<std::sync::Mutex<VecDeque<proto::FileChange>>>,
}

impl Iterator for ChangeIterator {
    type Item = proto::FileChange;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.lock().unwrap().pop_front()
    }
}

#[cfg(test)]
mod tests {
    fn is_send<T: Send>(_t: T) {}

    #[test]
    fn batch_stream_is_send() {
        let list = super::List::new();
        let stream = list.change_batches();
        is_send(stream);
    }

    #[test]
    fn update_is_send() {
        let list = super::List::new();
        let fut = list.update(std::iter::empty());
        is_send(fut);
    }
}
