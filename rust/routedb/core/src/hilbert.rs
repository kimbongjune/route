/// 격자 크기 2^ORDER x 2^ORDER
const ORDER: u32 = 16;

/// 2^order x 2^order 격자의 (x, y)를 힐베르트 곡선 위의 순번으로 변환한다.
pub fn index(order: u32, x: u32, y: u32) -> u64 {
    let n = 1u32 << order;
    let (mut x, mut y) = (x, y);
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u32::from(x & s > 0);
        let ry = u32::from(y & s > 0);
        d += u64::from(s) * u64::from(s) * u64::from((3 * rx) ^ ry);
        // 다음 단계 사분면 방향에 맞게 좌표를 회전한다
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    d
}

/// 좌표 목록을 힐베르트 곡선 순서로 정렬했을 때의 원래 위치 목록을 반환한다.
pub fn sort_order(points: &[(f64, f64)]) -> Vec<usize> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in points {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    // 전체 범위를 격자 칸 번호로 변환
    let cells = f64::from((1u32 << ORDER) - 1);
    let to_cell = |v: f64, min: f64, max: f64| ((v - min) / (max - min).max(1e-9) * cells) as u32;

    let keys: Vec<u64> = points.iter().map(|&(x, y)| index(ORDER, to_cell(x, x0, x1), to_cell(y, y0, y1))).collect();
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order1() {
        // 2x2 격자는 (0,0) -> (0,1) -> (1,1) -> (1,0) 순서
        assert_eq!([index(1, 0, 0), index(1, 0, 1), index(1, 1, 1), index(1, 1, 0)], [0, 1, 2, 3]);
    }

    #[test]
    fn next_index_is_neighbor_cell() {
        // 순번이 이어지는 칸끼리 붙어 있는지 확인
        let order = 4;
        let n = 1u32 << order;
        let mut cells = vec![(0, 0); (n * n) as usize];
        for x in 0..n {
            for y in 0..n {
                cells[index(order, x, y) as usize] = (x, y);
            }
        }
        for w in cells.windows(2) {
            assert_eq!(w[0].0.abs_diff(w[1].0) + w[0].1.abs_diff(w[1].1), 1);
        }
    }
}
