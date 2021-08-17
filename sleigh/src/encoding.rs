pub trait Encoding {
    type Error: std::error::Error;

    fn serialize<S: ?Sized + serde::Serialize>(t: &S) -> Result<Vec<u8>, Self::Error>;

    fn deserialize<'a, T: serde::Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, Self::Error>;
}

mod bincode {
    use bincode::Options;
    use lazy_static::lazy_static;

    // WART bincode makes this very, very, ugly.
    // At least this way, the compiler yells at me if I get the repeated things wrong.
    // https://github.com/bincode-org/bincode/issues/399
    type MyBincodeOptions = bincode::config::WithOtherEndian<
        bincode::config::WithOtherIntEncoding<
            bincode::config::WithOtherTrailing<
                bincode::config::WithOtherLimit<
                    bincode::config::DefaultOptions,
                    bincode::config::Bounded,
                >,
                bincode::config::RejectTrailing,
            >,
            bincode::config::VarintEncoding,
        >,
        bincode::config::BigEndian,
    >;
    lazy_static! {
        static ref BINCODE_OPTIONS: MyBincodeOptions = bincode::DefaultOptions::new()
            .with_limit(10000)
            .reject_trailing_bytes()
            .with_varint_encoding()
            .with_big_endian();
    }

    pub struct Bincode {}

    impl super::Encoding for Bincode {
        type Error = bincode::Error;

        fn serialize<S: ?Sized + serde::Serialize>(t: &S) -> bincode::Result<Vec<u8>> {
            BINCODE_OPTIONS.serialize(t)
        }

        fn deserialize<'a, T: serde::Deserialize<'a>>(bytes: &'a [u8]) -> bincode::Result<T> {
            BINCODE_OPTIONS.deserialize(bytes)
        }
    }
}
pub use self::bincode::Bincode;
