use std::{future::Future, pin::Pin, sync::mpsc, time::Duration};

// TODO make keys typed too.

pub mod encoding;

pub enum MergeVerdict {
    Keep,
    Remove,
}
pub trait Merge<U> {
    fn merge(&mut self, update: U) -> MergeVerdict;
}

pub struct Tree<V, U, Enc: self::encoding::Encoding> {
    tree: sled::Tree,
    _phantom_v: std::marker::PhantomData<V>,
    _phantom_u: std::marker::PhantomData<U>,
    _phantom_enc: std::marker::PhantomData<Enc>,
}

#[derive(thiserror::Error, Debug)]
pub enum InsertError<SerializeError: 'static + std::error::Error> {
    #[error("error serializing: {0}")]
    Serialize(#[source] SerializeError),

    #[error("database error: {0}")]
    DB(#[from] sled::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum GetError<DeserializeError: 'static + std::error::Error> {
    #[error("error deserializing: {0}")]
    Deserialize(#[source] DeserializeError),

    #[error("database error: {0}")]
    DB(#[from] sled::Error),
}

impl<V, U, Enc: 'static + self::encoding::Encoding> Tree<V, U, Enc>
where
    // TODO do i really have to repeat the constraints here?
    V: 'static
        + ?Sized
        + serde::Serialize
        + for<'de> serde::de::Deserialize<'de>
        + Default
        + Merge<U>,
    U: 'static + ?Sized + serde::Serialize + for<'de> serde::de::Deserialize<'de>,
{
    pub fn new(tree: sled::Tree) -> Self {
        tree.set_merge_operator(Self::merge_operator);
        Tree {
            tree,
            _phantom_v: std::marker::PhantomData,
            _phantom_u: std::marker::PhantomData,
            _phantom_enc: std::marker::PhantomData,
        }
    }

    fn merge_operator(key: &[u8], old: Option<&[u8]>, new: &[u8]) -> Option<Vec<u8>> {
        let mut item = match old {
            None => V::default(),
            Some(b) => {
                let item: V = Enc::deserialize(b).unwrap_or_else(|error| {
                    panic!("database has corrupt item: key={:?}: {}", &key, error)
                });
                item
            }
        };
        let ops: U = Enc::deserialize(new).unwrap_or_else(|error| {
            panic!("database has corrupt item (#2): key={:?}: {}", &key, error)
        });
        match item.merge(ops) {
            MergeVerdict::Remove => None,
            MergeVerdict::Keep => {
                let buf = Enc::serialize(&item)
                    .unwrap_or_else(|error| panic!("cannot serialize: {}", error));
                Some(buf)
            }
        }
    }

    pub fn insert<K>(&self, key: K, item: &V) -> Result<(), InsertError<Enc::Error>>
    where
        K: AsRef<[u8]>,
    {
        let buf = Enc::serialize(item).map_err(InsertError::Serialize)?;
        let _ = self.tree.insert(key, buf)?;
        // For now, we don't bother with returning any old value.
        // That would require a lazy deserialize wrapper.
        Ok(())
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<V>, GetError<Enc::Error>> {
        match self.tree.get(key)? {
            None => Ok(None),
            Some(buf) => {
                let item: V = Enc::deserialize(&buf).map_err(GetError::Deserialize)?;
                Ok(Some(item))
            }
        }
    }

    pub fn merge<K>(&self, key: K, update: &U) -> Result<(), InsertError<Enc::Error>>
    where
        K: AsRef<[u8]>,
    {
        let buf = Enc::serialize(update).map_err(InsertError::Serialize)?;
        let _ = self.tree.merge(key, buf)?;
        // For now, we don't bother with returning any old value.
        // That would require a lazy deserialize wrapper.
        Ok(())
    }

    pub fn scan_prefix<P>(&self, prefix: P) -> Iter<V, Enc>
    where
        P: AsRef<[u8]>,
    {
        let iter = self.tree.scan_prefix(prefix);
        Iter {
            iter,
            _phantom_v: std::marker::PhantomData,
            _phantom_enc: std::marker::PhantomData,
        }
    }

    pub fn watch_prefix<P: AsRef<[u8]>>(&self, prefix: P) -> Subscriber<V, Enc> {
        let sub = self.tree.watch_prefix(prefix);
        Subscriber {
            sub,
            _phantom_v: std::marker::PhantomData,
            _phantom_enc: std::marker::PhantomData,
        }
    }
}

pub struct Iter<V, Enc: self::encoding::Encoding> {
    iter: sled::Iter,
    _phantom_v: std::marker::PhantomData<V>,
    _phantom_enc: std::marker::PhantomData<Enc>,
}

impl<V, Enc: self::encoding::Encoding + 'static> Iterator for Iter<V, Enc>
where
    // TODO do i really have to repeat the constraints here?
    V: 'static + ?Sized + for<'de> serde::de::Deserialize<'de>,
{
    type Item = Result<(sled::IVec, V), GetError<Enc::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        // The layering of Option<Result< makes this awkward code.
        let result = match self.iter.next() {
            None => return None,
            Some(r) => r,
        };
        let (key, buf) = match result {
            Err(error) => return Some(Err(GetError::DB(error))),
            Ok((key, buf)) => (key, buf),
        };
        let dec: Result<V, Enc::Error> = Enc::deserialize(&buf);
        match dec {
            Err(error) => return Some(Err(GetError::Deserialize(error))),
            Ok(item) => Some(Ok((key, item))),
        }
    }
}

pub struct Subscriber<V, Enc: self::encoding::Encoding> {
    sub: sled::Subscriber,
    _phantom_v: std::marker::PhantomData<V>,
    _phantom_enc: std::marker::PhantomData<Enc>,
}

pub enum Event<V> {
    Insert { key: sled::IVec, value: V },
    Remove { key: sled::IVec },
}

#[derive(thiserror::Error, Debug)]
pub enum SubscriberTimeoutError<DeserializeError: 'static + std::error::Error> {
    #[error("error deserializing: {0}")]
    Deserialize(#[source] DeserializeError),

    #[error(transparent)]
    Recv(#[from] mpsc::RecvTimeoutError),
}

impl<V, Enc: self::encoding::Encoding + 'static> Subscriber<V, Enc>
where
    // TODO do i really have to repeat the constraints here?
    V: 'static + ?Sized + for<'de> serde::de::Deserialize<'de>,
{
    fn event_from_sled(orig: sled::Event) -> Result<Event<V>, Enc::Error> {
        let event = match orig {
            sled::Event::Insert { key, value } => {
                let item: V = Enc::deserialize(&value)?;
                Event::Insert { key, value: item }
            }
            sled::Event::Remove { key } => Event::Remove { key },
        };
        Ok(event)
    }

    pub fn next_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Event<V>, SubscriberTimeoutError<Enc::Error>> {
        let orig = self.sub.next_timeout(timeout)?;
        let event = Self::event_from_sled(orig).map_err(SubscriberTimeoutError::Deserialize)?;
        Ok(event)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SubscriberIteratorError<DeserializeError: 'static + std::error::Error> {
    #[error("error deserializing: {0}")]
    Deserialize(#[source] DeserializeError),
}

impl<V, Enc: self::encoding::Encoding + 'static> Iterator for Subscriber<V, Enc>
where
    V: 'static + for<'de> serde::de::Deserialize<'de>,
{
    // TODO no RecvTimeoutError here
    type Item = Result<Event<V>, SubscriberIteratorError<Enc::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.sub
            .next()
            .map(|orig| Self::event_from_sled(orig).map_err(SubscriberIteratorError::Deserialize))
    }
}

impl<V, Enc: self::encoding::Encoding + 'static> Future for Subscriber<V, Enc>
where
    V: 'static + for<'de> serde::de::Deserialize<'de>,
{
    type Output = Option<Result<Event<V>, SubscriberIteratorError<Enc::Error>>>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let sub = unsafe { self.map_unchecked_mut(|s| &mut s.sub) };
        let poll = sub.poll(cx);
        match poll {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Ready(Some(orig)) => {
                let result =
                    Self::event_from_sled(orig).map_err(SubscriberIteratorError::Deserialize);
                std::task::Poll::Ready(Some(result))
            }
        }
    }
}
