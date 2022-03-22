// Control JSON formatting of duration.
//
// https://serde.rs/custom-date-format.html

use serde::Deserialize;
use std::time;

pub fn serialize<S>(dur: &time::Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_f64(dur.as_secs_f64())
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<time::Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let f = f64::deserialize(deserializer)?;
    // std::time::Duration::from_secs_f64 panics on +-Inf, <0, overflows;
    // this utterly sucks.
    //
    // https://users.rust-lang.org/t/duration-from-float-without-panic/47528/11
    // https://doc.rust-lang.org/edition-guide/rust-2018/error-handling-and-panics/controlling-panics-with-std-panic.html
    // https://doc.rust-lang.org/std/panic/fn.catch_unwind.html
    let result = std::panic::catch_unwind(|| time::Duration::from_secs_f64(f));
    result.map_err(|_| {
        // preserving the panic message would be nice, but I have no idea where to stuff it in serde errors, unless I use `custom`.
        serde::de::Error::invalid_value(serde::de::Unexpected::Float(f), &"seconds as float")
    })
}
