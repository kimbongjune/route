//! 이름과 검색어의 정규화, 2-gram 키

/// 검색용으로 정규화한다. 소문자로 바꾸고 공백, 괄호, 가운뎃점, 쉼표, 밑줄, 따옴표를 뺀다
pub fn normalize_into(s: &str, out: &mut Vec<char>) {
    out.clear();
    for c in s.chars() {
        if c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '·' | ',' | '_' | '"' | '\'') {
            continue;
        }
        out.extend(c.to_lowercase());
    }
}

pub fn normalize(s: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    normalize_into(s, &mut out);
    out
}

/// 기본 다국어 평면 밖의 글자는 U+FFFD로 본다
fn unit(c: char) -> u32 {
    let u = c as u32;
    if u > 0xFFFF { 0xFFFD } else { u }
}

/// 두 글자를 하나의 키로 만든다
pub fn key(a: char, b: char) -> u32 {
    (unit(a) << 16) | unit(b)
}

/// 정규화한 글자열의 2-gram 키. 두 글자보다 짧으면 비어 있다
pub fn keys(chars: &[char]) -> Vec<u32> {
    chars.windows(2).map(|w| key(w[0], w[1])).collect()
}

/// hay 안에 needle이 이어서 나오는지
pub fn contains(hay: &[char], needle: &[char]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

/// 두 점 사이 거리(m). 짧은 거리용 등장방형 근사
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let lat = (a.0 + b.0).to_radians() / 2.0;
    let dy = (a.0 - b.0).to_radians() * 6_371_000.0;
    let dx = (a.1 - b.1).to_radians() * 6_371_000.0 * lat.cos();
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_keys() {
        let n = normalize("CJ 올리브영 (광주선운지구점)");
        assert_eq!(n.iter().collect::<String>(), "cj올리브영광주선운지구점");
        assert_eq!(
            keys(&normalize("서울역")),
            [key('서', '울'), key('울', '역')]
        );
        assert!(keys(&normalize("역")).is_empty());
        assert!(contains(&n, &normalize("선운")));
        assert!(!contains(&n, &normalize("선운점")));
        assert!(contains(&n, &[]));
    }

    #[test]
    fn distance() {
        // 서울역과 서울시청은 약 1.7km
        let d = distance_m((37.5547, 126.9707), (37.5666, 126.9784));
        assert!((1_400.0..1_600.0).contains(&d), "{d}");
    }
}
