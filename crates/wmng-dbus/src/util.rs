use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

use crate::{Error, Result};

pub(crate) type PropertyMap = HashMap<String, OwnedValue>;
pub(crate) type InterfaceMap = HashMap<String, PropertyMap>;

pub(crate) fn required<T>(
    props: &PropertyMap,
    interface: &'static str,
    property: &'static str,
) -> Result<T>
where
    T: TryFrom<OwnedValue>,
{
    let value = props
        .get(property)
        .cloned()
        .ok_or(Error::MissingProperty {
            interface,
            property,
        })?;
    convert(value, interface, property)
}

pub(crate) fn optional<T>(
    props: &PropertyMap,
    interface: &'static str,
    property: &'static str,
) -> Result<Option<T>>
where
    T: TryFrom<OwnedValue>,
{
    props.get(property)
        .cloned()
        .map(|value| convert(value, interface, property))
        .transpose()
}

fn convert<T>(value: OwnedValue, interface: &'static str, property: &'static str) -> Result<T>
where
    T: TryFrom<OwnedValue>,
{
    value.try_into().map_err(|_| Error::InvalidProperty {
        interface,
        property,
        reason: "unexpected D-Bus type",
    })
}

pub(crate) fn trim_nul(bytes: &[u8]) -> &[u8] {
    match bytes.iter().position(|byte| *byte == 0) {
        Some(index) => &bytes[..index],
        None => bytes,
    }
}

pub(crate) fn bytes_to_string(
    bytes: Vec<u8>,
    interface: &'static str,
    property: &'static str,
) -> Result<String> {
    String::from_utf8(trim_nul(&bytes).to_vec()).map_err(|_| Error::InvalidProperty {
        interface,
        property,
        reason: "invalid UTF-8",
    })
}

pub(crate) fn normalize_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty() && trimmed != "/").then(|| trimmed.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize_string, trim_nul};

    #[test]
    fn trim_nul_stops_at_first_terminator() {
        assert_eq!(trim_nul(b"/dev/sda1\0ignored"), b"/dev/sda1");
    }

    #[test]
    fn normalize_string_discards_empty_and_root_sentinel() {
        assert_eq!(normalize_string(Some(String::new())), None);
        assert_eq!(normalize_string(Some(" / ".to_string())), None);
        assert_eq!(
            normalize_string(Some("/org/freedesktop/UDisks2/drives/usb".to_string())),
            Some("/org/freedesktop/UDisks2/drives/usb".to_string())
        );
    }
}
