/// 가중치 프로파일. line graph 통행마다 CCH 가중치(u32)를 만든다.
///
/// - 거리: 통행 비용(m)을 cm로 반올림한다.
/// - 최소시간: 링크 길이를 제한속도 MAX_SPD로 나눈 통행 시간(ms)의 합이다. 건너가는 통행의 거리는 다음 링크 속도로 계산한다.
/// - 무료도로: 최소시간에서 고속국도 링크와 요금소 링크로 들어가는 통행을 INF_WEIGHT로 막는다.
pub struct ProfileInput<'a> {
    pub turn_first_out: &'a [u32],
    pub turn_head: &'a [u32],
    /// 통행 비용(m)
    pub turn_weight: &'a [f64],
    pub via_first: &'a [u32],
    pub via: &'a [u32],
    /// 링크 길이(m)
    pub length: &'a [f64],
    /// 링크 제한속도(km/h)
    pub max_spd: &'a [u16],
    pub toll_first: &'a [u32],
    pub toll_unit: &'a [u32],
    /// 링크 도로등급
    pub road_rank: &'a [u8],
}

/// 고속국도 도로등급
pub const EXPRESSWAY: u8 = 101;

/// 링크 e의 통행 시간(ms)
pub fn link_time_ms(length: &[f64], max_spd: &[u16], e: usize) -> f64 {
    length[e] / (max_spd[e] as f64 / 3.6) * 1000.0
}

/// 통행 비용(m)을 cm로 반올림한 거리 가중치
pub fn distance_weights(turn_weight: &[f64]) -> Vec<u32> {
    turn_weight
        .iter()
        .map(|&m| (m * 100.0).round() as u32)
        .collect()
}

impl ProfileInput<'_> {
    fn tolls_of(&self, e: u32) -> &[u32] {
        &self.toll_unit
            [self.toll_first[e as usize] as usize..self.toll_first[e as usize + 1] as usize]
    }

    /// 통행마다 (출발 링크, 경유 링크와 다음 링크 열)로 f를 호출하여 가중치를 만든다.
    fn map_turns(&self, mut f: impl FnMut(u32, &[u32], usize) -> u32) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.turn_head.len());
        let mut seq = Vec::new();
        for a in 0..self.turn_first_out.len() - 1 {
            for t in self.turn_first_out[a] as usize..self.turn_first_out[a + 1] as usize {
                seq.clear();
                seq.extend_from_slice(
                    &self.via[self.via_first[t] as usize..self.via_first[t + 1] as usize],
                );
                seq.push(self.turn_head[t]);
                out.push(f(a as u32, &seq, t));
            }
        }
        out
    }

    /// 통행 t의 시간(ms). 경유 링크와 다음 링크의 시간 합에 건너가는 거리의 시간을 더한다.
    fn turn_time_ms(&self, seq: &[u32], t: usize) -> f64 {
        let gap = self.turn_weight[t] - seq.iter().map(|&e| self.length[e as usize]).sum::<f64>();
        let head = *seq.last().unwrap() as usize;
        seq.iter()
            .map(|&e| link_time_ms(self.length, self.max_spd, e as usize))
            .sum::<f64>()
            + gap / (self.max_spd[head] as f64 / 3.6) * 1000.0
    }

    /// 최소시간 가중치(ms)
    pub fn time_weights(&self) -> Vec<u32> {
        self.map_turns(|_, seq, t| self.turn_time_ms(seq, t).round() as u32)
    }

    /// 무료도로 가중치(ms). 고속국도 링크나 요금소 링크로 들어가는 통행은 INF_WEIGHT이다.
    pub fn free_weights(&self) -> Vec<u32> {
        self.map_turns(|_, seq, t| {
            if seq
                .iter()
                .any(|&e| self.road_rank[e as usize] == EXPRESSWAY || !self.tolls_of(e).is_empty())
            {
                cch::INF_WEIGHT
            } else {
                self.turn_time_ms(seq, t).round() as u32
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 링크 4개: 0 (100m, 60km/h, 시군도), 1 (50m, 30km/h, 시군도, 영업소 246), 2 (100m, 60km/h, 고속국도), 3 (100m, 60km/h, 시군도).
    /// 통행 0→1은 경유 없음, 0→2는 1을 경유하고 10m 건너간다, 1→2와 0→3은 경유 없음
    fn demo() -> (
        Vec<u32>,
        Vec<u32>,
        Vec<f64>,
        Vec<u32>,
        Vec<u32>,
        Vec<f64>,
        Vec<u16>,
        Vec<u32>,
        Vec<u32>,
        Vec<u8>,
    ) {
        (
            vec![0, 3, 4, 4, 4],
            vec![1, 2, 3, 2],
            vec![50.0, 160.0, 100.0, 100.0],
            vec![0, 0, 1, 1, 1],
            vec![1],
            vec![100.0, 50.0, 100.0, 100.0],
            vec![60, 30, 60, 60],
            vec![0, 0, 1, 1, 1],
            vec![246],
            vec![107, 107, 101, 107],
        )
    }

    #[test]
    fn time_and_free_weights() {
        let d = demo();
        let input = ProfileInput {
            turn_first_out: &d.0,
            turn_head: &d.1,
            turn_weight: &d.2,
            via_first: &d.3,
            via: &d.4,
            length: &d.5,
            max_spd: &d.6,
            toll_first: &d.7,
            toll_unit: &d.8,
            road_rank: &d.9,
        };
        // 링크 1은 50m를 30km/h로 6,000ms, 링크 2·3은 100m를 60km/h로 6,000ms, 건너가는 10m는 60km/h로 600ms
        assert_eq!(input.time_weights(), [6_000, 12_600, 6_000, 6_000]);
        // 통행 순서는 0→1, 0→2, 0→3, 1→2이다. 0→1은 요금소 링크로, 0→2는 요금소 링크를 경유해 고속국도로, 1→2는 고속국도로 들어가므로 막히고 0→3만 남는다
        assert_eq!(
            input.free_weights(),
            [cch::INF_WEIGHT, cch::INF_WEIGHT, 6_000, cch::INF_WEIGHT]
        );
        assert_eq!(distance_weights(&d.2), [5_000, 16_000, 10_000, 10_000]);
    }
}
