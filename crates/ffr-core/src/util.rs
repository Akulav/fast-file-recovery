//! Small bounds-checked helpers for reading integers out of untrusted byte slices.

#[inline]
pub fn u8_at(b: &[u8], off: usize) -> Option<u8> {
    b.get(off).copied()
}

macro_rules! int_reader {
    ($name:ident, $t:ty, $n:expr, $conv:ident) => {
        #[inline]
        pub fn $name(b: &[u8], off: usize) -> Option<$t> {
            let s = b.get(off..off.checked_add($n)?)?;
            let mut a = [0u8; $n];
            a.copy_from_slice(s);
            Some(<$t>::$conv(a))
        }
    };
}

int_reader!(le16, u16, 2, from_le_bytes);
int_reader!(be16, u16, 2, from_be_bytes);
int_reader!(le32, u32, 4, from_le_bytes);
int_reader!(be32, u32, 4, from_be_bytes);
int_reader!(le64, u64, 8, from_le_bytes);
int_reader!(be64, u64, 8, from_be_bytes);

/// Decode UTF-16LE bytes, stopping at the first NUL.
pub fn utf16le(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Windows FILETIME (100 ns since 1601) to Unix seconds.
pub fn filetime_to_unix(ft: u64) -> Option<i64> {
    if ft == 0 {
        return None;
    }
    let secs = (ft / 10_000_000) as i64 - 11_644_473_600;
    (0..=4_102_444_800).contains(&secs).then_some(secs)
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date from days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// DOS date + time (FAT/exFAT, local time treated as UTC) to Unix seconds.
pub fn dos_to_unix(date: u16, time: u16) -> Option<i64> {
    if date == 0 {
        return None;
    }
    let y = 1980 + (date >> 9) as i64;
    let m = ((date >> 5) & 0xF) as u32;
    let d = (date & 0x1F) as u32;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let h = (time >> 11) as i64;
    let mi = ((time >> 5) & 0x3F) as i64;
    let s = ((time & 0x1F) * 2) as i64;
    Some(days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60 + s)
}

/// Current UTC time as `(year, month, day, hour, minute, second)`.
pub fn now_utc() -> (i64, u32, u32, u32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem / 60) % 60) as u32,
        (rem % 60) as u32,
    )
}

/// Human-readable byte size ("1.5 GB").
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

/// Round `v` up to a multiple of `align` (power of two not required).
#[inline]
pub fn align_up(v: u64, align: u64) -> u64 {
    if align <= 1 {
        v
    } else {
        v.div_ceil(align) * align
    }
}

#[inline]
pub fn align_down(v: u64, align: u64) -> u64 {
    if align <= 1 {
        v
    } else {
        v / align * align
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip() {
        for z in [-1000, 0, 1, 365, 10_957, 19_000, 30_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(days_from_civil(2000, 1, 1), 10_957);
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1_500_000), "1.50 MB");
        assert_eq!(align_up(513, 512), 1024);
        assert_eq!(align_down(1023, 512), 512);
    }

    #[test]
    fn readers_are_bounds_checked() {
        let b = [1u8, 2, 3];
        assert_eq!(le16(&b, 1), Some(0x0302));
        assert_eq!(le32(&b, 0), None);
        assert_eq!(le16(&b, usize::MAX), None);
    }
}
