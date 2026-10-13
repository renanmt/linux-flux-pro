pub mod hid;
pub mod sensors;

/// Temperatures are integer millidegrees, avoiding float rounding at digit boundaries.
pub fn digits(value: Option<i64>) -> [u8; 3] {
    match value {
        Some(value @ 0..=99_900) => {
            let tenths = value / 100;
            [
                (tenths / 100) as u8,
                ((tenths / 10) % 10) as u8,
                (tenths % 10) as u8,
            ]
        }
        _ => [0xee; 3],
    }
}

pub fn report(cpu: Option<i64>, gpu: Option<i64>) -> [u8; 65] {
    let mut output = [0; 65];
    output[1..6].copy_from_slice(&[0x55, 0xaa, 1, 1, 6]);
    output[6..9].copy_from_slice(&digits(cpu));
    output[9..12].copy_from_slice(&digits(gpu));
    output[12] = output[1..12]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    output
}

/// Log failures only on transitions, while continuing to retry every update.
#[derive(Default)]
pub struct Health {
    last_error: Option<String>,
}

impl Health {
    pub fn observe<T>(&mut self, name: &str, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(value) => {
                if self.last_error.take().is_some() {
                    eprintln!("{name} recovered");
                }
                Some(value)
            }
            Err(error) => {
                if self.last_error.as_ref() != Some(&error) {
                    eprintln!("{name} unavailable: {error}");
                    self.last_error = Some(error);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_packet_and_padding() {
        let packet = report(Some(42_500), Some(63_200));
        assert_eq!(
            &packet[..13],
            &[0, 0x55, 0xaa, 1, 1, 6, 4, 2, 5, 6, 3, 2, 0x1d]
        );
        assert_eq!(&packet[13..], &[0; 52]);
    }

    #[test]
    fn missing_and_extreme_values_never_wrap() {
        assert_eq!(report(None, None)[12], 0x9b);
        for value in [None, Some(-1), Some(99_901), Some(100_000), Some(i64::MAX)] {
            assert_eq!(digits(value), [0xee; 3]);
        }
        assert_eq!(digits(Some(0)), [0, 0, 0]);
        assert_eq!(digits(Some(99_900)), [9, 9, 9]);
        assert_eq!(digits(Some(63_299)), [6, 3, 2]);
    }

    #[test]
    fn all_displayable_tenths_round_trip() {
        for value in 0..=999 {
            let d = digits(Some(value * 100));
            assert_eq!(
                i64::from(d[0]) * 100 + i64::from(d[1]) * 10 + i64::from(d[2]),
                value
            );
            let r = report(Some(value * 100), None);
            assert_eq!(r[12], r[1..12].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
        }
    }
}
