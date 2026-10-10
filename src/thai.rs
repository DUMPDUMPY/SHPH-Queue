//! Thai number words and the voice-clip sequence for an announcement.

const DIGITS: [&str; 10] = ["ศูนย์", "หนึ่ง", "สอง", "สาม", "สี่", "ห้า", "หก", "เจ็ด", "แปด", "เก้า"];

/// Read a number (0–999) the way Thai speakers say it: 11 = สิบเอ็ด, 21 = ยี่สิบเอ็ด, 101 = หนึ่งร้อยเอ็ด.
pub fn number_words(n: u32) -> String {
    if n == 0 {
        return DIGITS[0].to_string();
    }
    let n = n.min(999);
    let (h, t, u) = (n / 100, (n % 100) / 10, n % 10);
    let mut s = String::new();
    if h > 0 {
        s.push_str(DIGITS[h as usize]);
        s.push_str("ร้อย");
    }
    match t {
        0 => {}
        1 => s.push_str("สิบ"),
        2 => s.push_str("ยี่สิบ"),
        _ => {
            s.push_str(DIGITS[t as usize]);
            s.push_str("สิบ");
        }
    }
    if u > 0 {
        if u == 1 && (t > 0 || h > 0) {
            s.push_str("เอ็ด");
        } else {
            s.push_str(DIGITS[u as usize]);
        }
    }
    s
}

/// Voice-clip names (without extension) that read a number aloud.
pub fn number_clips(n: u32) -> Vec<String> {
    let n = n.clamp(1, 999);
    let (h, r) = (n / 100, n % 100);
    let mut v = Vec::new();
    if h > 0 {
        v.push(format!("hundred_{h}"));
        if r == 1 {
            v.push("ed".to_string());
        } else if r > 0 {
            v.push(format!("num_{r}"));
        }
    } else {
        v.push(format!("num_{r}"));
    }
    v
}

/// Built-in phrases a room can use in its announcement.
pub const PHRASES: &[(&str, &str)] = &[
    ("exam", "ที่ห้องตรวจ"),
    ("pharmacy", "รับยาที่ห้องยา"),
    ("room", "ที่ห้อง"),
    ("counter", "ที่ช่องบริการ"),
    ("screening", "ที่จุดคัดกรอง"),
    ("lab", "ที่ห้องเจาะเลือด"),
    ("dental", "ที่ห้องทันตกรรม"),
];

pub fn phrase_text(key: &str) -> &'static str {
    PHRASES.iter().find(|(k, _)| *k == key).map(|(_, t)| *t).unwrap_or("ที่ห้อง")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(number_words(1), "หนึ่ง");
        assert_eq!(number_words(10), "สิบ");
        assert_eq!(number_words(11), "สิบเอ็ด");
        assert_eq!(number_words(12), "สิบสอง");
        assert_eq!(number_words(20), "ยี่สิบ");
        assert_eq!(number_words(21), "ยี่สิบเอ็ด");
        assert_eq!(number_words(99), "เก้าสิบเก้า");
        assert_eq!(number_words(100), "หนึ่งร้อย");
        assert_eq!(number_words(101), "หนึ่งร้อยเอ็ด");
        assert_eq!(number_words(115), "หนึ่งร้อยสิบห้า");
        assert_eq!(number_words(250), "สองร้อยห้าสิบ");
    }

    #[test]
    fn clips() {
        assert_eq!(number_clips(12), vec!["num_12"]);
        assert_eq!(number_clips(100), vec!["hundred_1"]);
        assert_eq!(number_clips(101), vec!["hundred_1", "ed"]);
        assert_eq!(number_clips(245), vec!["hundred_2", "num_45"]);
    }
}
