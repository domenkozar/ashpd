//! Wire adapters for the public enumflags2 flag sets.
//!
//! zbus 6 no longer implements its wire traits for enumflags2. Keep portal
//! flag APIs unchanged and encode their u32 representation at the D-Bus boundary.

use enumflags2::{BitFlag, BitFlags};
use serde::{Deserialize, Serialize};
use zbus::Type;

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub(crate) struct WireFlags<F: BitFlag<Numeric = u32>>(pub BitFlags<F>);

impl<F: BitFlag<Numeric = u32>> Type for WireFlags<F> {
    const SIGNATURE: &'static zbus::Signature = u32::SIGNATURE;
}

#[cfg(any(test, feature = "input_capture"))]
pub(crate) fn serialize<F, S>(value: &BitFlags<F>, serializer: S) -> Result<S::Ok, S::Error>
where
    F: BitFlag<Numeric = u32>,
    S: serde::Serializer,
{
    zbus::as_variant::Serialize(&WireFlags(*value)).serialize(serializer)
}

#[cfg(any(test, feature = "input_capture", feature = "remote_desktop"))]
pub(crate) fn deserialize<'de, F, D>(deserializer: D) -> Result<BitFlags<F>, D::Error>
where
    F: BitFlag<Numeric = u32>,
    D: serde::Deserializer<'de>,
{
    zbus::as_variant::Deserialize::<WireFlags<F>>::deserialize(deserializer).map(|v| v.0.0)
}

#[cfg(any(
    test,
    feature = "flatpak",
    feature = "screencast",
    feature = "remote_desktop"
))]
pub(crate) mod optional {
    use super::*;

    pub(crate) fn serialize<F, S>(
        value: &Option<BitFlags<F>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        F: BitFlag<Numeric = u32>,
        S: serde::Serializer,
    {
        zbus::as_variant::optional::serialize(&value.map(WireFlags), serializer)
    }

    #[cfg(any(test, feature = "screencast"))]
    pub(crate) fn deserialize<'de, F, D>(deserializer: D) -> Result<Option<BitFlags<F>>, D::Error>
    where
        F: BitFlag<Numeric = u32>,
        D: serde::Deserializer<'de>,
    {
        zbus::as_variant::optional::deserialize::<WireFlags<F>, D>(deserializer)
            .map(|value| value.map(|v| v.0))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use zbus::{
        OwnedVariant,
        wire::{LE, serialized::Context, to_bytes},
    };

    use super::*;

    #[enumflags2::bitflags]
    #[repr(u32)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Flag {
        First = 1,
        Second = 2,
    }

    #[derive(Debug, Serialize, Deserialize, Type)]
    #[zbus(signature = "dict")]
    struct Options {
        #[serde(with = "crate::flags")]
        required: BitFlags<Flag>,
        #[serde(
            default,
            with = "crate::flags::optional",
            skip_serializing_if = "Option::is_none"
        )]
        optional: Option<BitFlags<Flag>>,
    }

    #[test]
    fn flag_sets_keep_the_unsigned_wire_representation() {
        let context = Context::new(LE, 0);
        for bits in 0..=3 {
            let flags = WireFlags(BitFlags::<Flag>::from_bits(bits).unwrap());
            let encoded = to_bytes(context, &flags).unwrap();
            assert_eq!(encoded.deserialize::<u32>().unwrap().0, bits);
            let decoded: WireFlags<Flag> = encoded.deserialize().unwrap().0;
            assert_eq!(decoded.0, flags.0);
        }
        let invalid = to_bytes(context, &4_u32).unwrap();
        assert!(invalid.deserialize::<WireFlags<Flag>>().is_err());
    }

    #[test]
    fn flag_options_keep_variant_values_and_omit_absent_fields() {
        let context = Context::new(LE, 0);
        for optional in [None, Some(Flag::First | Flag::Second)] {
            let options = Options {
                required: Flag::Second.into(),
                optional,
            };
            let encoded = to_bytes(context, &options).unwrap();
            let dict: HashMap<String, OwnedVariant> = encoded.deserialize().unwrap().0;
            assert_eq!(u32::try_from(&dict["required"]).unwrap(), 2);
            assert_eq!(
                dict.get("optional").map(|v| u32::try_from(v).unwrap()),
                optional.map(|f| f.bits())
            );
            let decoded: Options = encoded.deserialize().unwrap().0;
            assert_eq!(decoded.required, options.required);
            assert_eq!(decoded.optional, options.optional);
        }
        let dict = HashMap::from([("required", OwnedVariant::from(4_u32))]);
        assert!(
            to_bytes(context, &dict)
                .unwrap()
                .deserialize::<Options>()
                .is_err()
        );
    }
}
