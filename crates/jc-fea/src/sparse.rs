//! 疎行列(CSR、行ごとに列の番号を昇順)と、その掛け算・転置・積。

/// 行ごとに列の番号を昇順に並べた疎行列。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Csr {
    pub rows: usize,
    pub cols: usize,
    pub ptr: Vec<usize>,
    pub idx: Vec<u32>,
    pub val: Vec<f64>,
}

/// これより大きな行列の掛け算は、いくつかのスレッドに分ける。
const PAR_ROWS: usize = 20_000;

fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8)
}

impl Csr {
    /// 形だけ(値は 0)の行列。`pattern[r]` は行 r の列の番号(昇順・重なり無し)。
    pub fn from_pattern(cols: usize, pattern: Vec<Vec<u32>>) -> Self {
        let mut ptr = Vec::with_capacity(pattern.len() + 1);
        ptr.push(0);
        let nnz: usize = pattern.iter().map(Vec::len).sum();
        let mut idx = Vec::with_capacity(nnz);
        for r in &pattern {
            idx.extend_from_slice(r);
            ptr.push(idx.len());
        }
        Csr {
            rows: pattern.len(),
            cols,
            ptr,
            idx,
            val: vec![0.0; nnz],
        }
    }

    /// 行 r・列 c の値の置き場所(形に無ければ `None`)。
    pub fn find(&self, r: usize, c: u32) -> Option<usize> {
        let s = &self.idx[self.ptr[r]..self.ptr[r + 1]];
        s.binary_search(&c).ok().map(|i| self.ptr[r] + i)
    }

    pub fn add(&mut self, r: usize, c: u32, v: f64) {
        let k = self.find(r, c).expect("組み立ての形に無い所へ足そうとした");
        self.val[k] += v;
    }

    pub fn diag(&self) -> Vec<f64> {
        (0..self.rows)
            .map(|r| self.find(r, r as u32).map_or(0.0, |k| self.val[k]))
            .collect()
    }

    /// y = A x
    pub fn mul(&self, x: &[f64], y: &mut [f64]) {
        let row = |r: usize| -> f64 {
            let mut s = 0.0;
            for k in self.ptr[r]..self.ptr[r + 1] {
                s += self.val[k] * x[self.idx[k] as usize];
            }
            s
        };
        if self.rows < PAR_ROWS {
            for (r, o) in y.iter_mut().enumerate() {
                *o = row(r);
            }
            return;
        }
        let t = threads();
        let chunk = self.rows.div_ceil(t);
        std::thread::scope(|s| {
            for (ci, part) in y.chunks_mut(chunk).enumerate() {
                let row = &row;
                s.spawn(move || {
                    for (i, o) in part.iter_mut().enumerate() {
                        *o = row(ci * chunk + i);
                    }
                });
            }
        });
    }

    pub fn transpose(&self) -> Csr {
        let mut count = vec![0usize; self.cols + 1];
        for &c in &self.idx {
            count[c as usize + 1] += 1;
        }
        for i in 0..self.cols {
            count[i + 1] += count[i];
        }
        let ptr = count.clone();
        let mut next = count;
        let mut idx = vec![0u32; self.idx.len()];
        let mut val = vec![0.0; self.val.len()];
        for r in 0..self.rows {
            for k in self.ptr[r]..self.ptr[r + 1] {
                let c = self.idx[k] as usize;
                idx[next[c]] = r as u32;
                val[next[c]] = self.val[k];
                next[c] += 1;
            }
        }
        Csr {
            rows: self.cols,
            cols: self.rows,
            ptr,
            idx,
            val,
        }
    }

    /// self + diag(w) B(行 r の B に w\[r\] を掛けて足す。形は両方を合わせた物)。
    pub fn add_scaled_rows(&self, b: &Csr, w: &[f64]) -> Csr {
        assert_eq!((self.rows, self.cols), (b.rows, b.cols));
        let mut ptr = vec![0usize];
        let mut idx = Vec::with_capacity(self.idx.len().max(b.idx.len()));
        let mut val = Vec::with_capacity(idx.capacity());
        for (r, wr) in w.iter().enumerate().take(self.rows) {
            let (mut i, ie) = (self.ptr[r], self.ptr[r + 1]);
            let (mut j, je) = (b.ptr[r], b.ptr[r + 1]);
            while i < ie || j < je {
                let ci = if i < ie { self.idx[i] } else { u32::MAX };
                let cj = if j < je { b.idx[j] } else { u32::MAX };
                if ci == cj {
                    idx.push(ci);
                    val.push(self.val[i] + wr * b.val[j]);
                    i += 1;
                    j += 1;
                } else if ci < cj {
                    idx.push(ci);
                    val.push(self.val[i]);
                    i += 1;
                } else {
                    idx.push(cj);
                    val.push(wr * b.val[j]);
                    j += 1;
                }
            }
            ptr.push(idx.len());
        }
        Csr {
            rows: self.rows,
            cols: self.cols,
            ptr,
            idx,
            val,
        }
    }

    /// A B(Gustavson の方法。大きい時は行を分けていくつかのスレッドで)。
    pub fn matmul(&self, b: &Csr) -> Csr {
        assert_eq!(self.cols, b.rows);
        let part = |rows: std::ops::Range<usize>| -> (Vec<usize>, Vec<u32>, Vec<f64>) {
            let mut len = Vec::with_capacity(rows.len());
            let mut idx = Vec::new();
            let mut val = Vec::new();
            let mut acc = vec![0.0; b.cols];
            let mut seen = vec![usize::MAX; b.cols];
            let mut cols: Vec<u32> = Vec::new();
            for r in rows {
                cols.clear();
                for k in self.ptr[r]..self.ptr[r + 1] {
                    let (m, a) = (self.idx[k] as usize, self.val[k]);
                    for kb in b.ptr[m]..b.ptr[m + 1] {
                        let c = b.idx[kb] as usize;
                        if seen[c] != r {
                            seen[c] = r;
                            acc[c] = 0.0;
                            cols.push(c as u32);
                        }
                        acc[c] += a * b.val[kb];
                    }
                }
                cols.sort_unstable();
                for &c in &cols {
                    idx.push(c);
                    val.push(acc[c as usize]);
                }
                len.push(cols.len());
            }
            (len, idx, val)
        };
        let parts: Vec<(Vec<usize>, Vec<u32>, Vec<f64>)> = if self.rows < PAR_ROWS / 4 {
            vec![part(0..self.rows)]
        } else {
            let t = threads();
            let chunk = self.rows.div_ceil(t);
            std::thread::scope(|s| {
                let hs: Vec<_> = (0..t)
                    .map(|i| {
                        let part = &part;
                        s.spawn(move || part(i * chunk..((i + 1) * chunk).min(self.rows)))
                    })
                    .collect();
                hs.into_iter()
                    .map(|h| h.join().expect("掛け算のスレッド"))
                    .collect()
            })
        };
        let mut ptr = Vec::with_capacity(self.rows + 1);
        ptr.push(0usize);
        let nnz: usize = parts.iter().map(|p| p.1.len()).sum();
        let mut idx = Vec::with_capacity(nnz);
        let mut val = Vec::with_capacity(nnz);
        for (len, i, v) in parts {
            for l in len {
                ptr.push(ptr.last().expect("最初は 0") + l);
            }
            idx.extend(i);
            val.extend(v);
        }
        Csr {
            rows: self.rows,
            cols: b.cols,
            ptr,
            idx,
            val,
        }
    }
}

pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn axpy(alpha: f64, x: &[f64], y: &mut [f64]) {
    for (yi, xi) in y.iter_mut().zip(x) {
        *yi += alpha * xi;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Csr {
        // [[2, 1, 0], [0, 3, 4]]
        let mut m = Csr::from_pattern(3, vec![vec![0, 1], vec![1, 2]]);
        m.add(0, 0, 2.0);
        m.add(0, 1, 1.0);
        m.add(1, 1, 3.0);
        m.add(1, 2, 4.0);
        m
    }

    #[test]
    fn transpose_and_product_agree_with_dense() {
        let a = small();
        let at = a.transpose();
        let p = a.matmul(&at); // 2×2: [[5, 3], [3, 25]]
        let dense: Vec<f64> = (0..2)
            .flat_map(|r| (0..2).map(move |c| (r, c)))
            .map(|(r, c)| p.find(r, c as u32).map_or(0.0, |k| p.val[k]))
            .collect();
        assert_eq!(dense, vec![5.0, 3.0, 3.0, 25.0]);
        let mut y = vec![0.0; 2];
        a.mul(&[1.0, 2.0, 3.0], &mut y);
        assert_eq!(y, vec![4.0, 18.0]);
    }
}
