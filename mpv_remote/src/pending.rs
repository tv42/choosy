use futures_channel::oneshot;
use slab::Slab;
use std::convert::TryInto;

pub struct Pending<T> {
    // slab is simple but has a tendency to reuse an ID immediately, which might annoy a human troubleshooter. alternative would be a u64 counter with wraparound and a hashmap, and just skip ids that are still in the map. however, i dislike code paths that'll only trigger after the heat death of the universe. that would avoid the awkwardness around id==0.

    // the slab, but only if we're not closed
    slab: Option<Slab<oneshot::Sender<T>>>,
}

impl<T> Pending<T> {
    pub fn new() -> Self {
        Self {
            slab: Some(Slab::new()),
        }
    }

    // If the u64 is None, we've already shut down.
    pub fn insert(&mut self) -> (Option<u64>, oneshot::Receiver<T>) {
        let (sender, receiver) = oneshot::channel();
        match &mut self.slab {
            Some(slab) => {
                // make sure we don't return id 0
                loop {
                    let entry = slab.vacant_entry();
                    let key = entry.key();
                    if key == 0 {
                        // insert something so it'll stay "allocated" so we don't see it again
                        let (dummy_sender, dummy_receiver) = oneshot::channel();
                        drop(dummy_receiver);
                        entry.insert(dummy_sender);
                        continue;
                    }
                    entry.insert(sender);
                    let id: u64 = key.try_into().expect("internal error: slab overflowed u64");
                    return (Some(id), receiver);
                }
            }
            None => {
                drop(sender);
                return (None, receiver);
            }
        }
    }

    pub fn get(&mut self, id: u64) -> Option<oneshot::Sender<T>> {
        let key: usize = match id.try_into() {
            Ok(k) => k,
            Err(_) => {
                // we could never have allocated this id
                return None;
            }
        };
        match &mut self.slab {
            None => None,
            Some(slab) => {
                // avoid panic from slab.remove on key not found
                if !slab.contains(key) {
                    return None;
                }
                let value = slab.remove(key);
                // TODO maybe the sending belongs here
                Some(value)
            }
        }
    }

    // Drop all pending operations and make them receive Canceled.
    // Any calls to insert after this will get Canceled.
    pub fn close(&mut self) {
        if let Some(slab) = &mut self.slab {
            slab.clear();
            self.slab = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[async_std::test]
    async fn simultaneous() {
        let mut p: Pending<String> = Pending::new();
        let (id1_option, mut recv1) = p.insert();
        let id1 = id1_option.expect("must get id1");
        let (id2_option, mut recv2) = p.insert();
        let id2 = id2_option.expect("must get id2");
        assert_ne!(id1, id2);
        assert_eq!(
            recv1.try_recv(),
            Ok(None),
            "recv1 must be not ready, but not canceled"
        );
        assert_eq!(
            recv2.try_recv(),
            Ok(None),
            "recv2 must be not ready, but not canceled"
        );

        let send2 = p.get(id2).expect("id2 must be found");
        assert!(p.get(id2).is_none(), "id2 must be no longer recognized");
        assert!(send2.is_connected_to(&recv2));
        assert_eq!(recv1.try_recv(), Ok(None));
        assert_eq!(recv2.try_recv(), Ok(None));
        send2
            .send("two".to_string())
            .expect("receiver 2 must be alive (in this test)");
        assert_eq!(recv1.try_recv(), Ok(None));
        assert_eq!(recv2.try_recv(), Ok(Some("two".to_string())));

        p.get(id1)
            .expect("id1 must be known")
            .send("one".to_string())
            .expect("receiver 1 must be alive (in this test)");
        assert_eq!(recv1.try_recv(), Ok(Some("one".to_string())));
    }

    #[async_std::test]
    async fn close_sends_goodbye() {
        let mut p: Pending<String> = Pending::new();
        let (_, mut recv1) = p.insert();
        let (_, mut recv2) = p.insert();

        p.close();
        assert_eq!(recv1.try_recv(), Err(oneshot::Canceled));
        assert_eq!(recv2.try_recv(), Err(oneshot::Canceled));
    }

    #[async_std::test]
    async fn insert_after_close() {
        let mut p: Pending<String> = Pending::new();

        p.close();
        let (id, mut recv) = p.insert();
        assert_eq!(id, None);
        assert_eq!(recv.try_recv(), Err(oneshot::Canceled));
    }
}
