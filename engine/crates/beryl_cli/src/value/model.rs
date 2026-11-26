//! The models `value-test` fits, hand-written: a logistic regression and a network of one
//! hidden layer of `tanh` units, each trained by Adam on the log loss with an L2 penalty, from
//! a fixed seed, stopping early on held-back samples.

use beryl_engine::rng::SplitMix64;
use beryl_search::sigmoid;

/// Samples in one Adam step.
const BATCH: usize = 64;
/// The most passes over the training samples.
const EPOCHS: usize = 300;
/// Passes with no better validation loss before training stops.
const PATIENCE: usize = 20;
const RATE: f64 = 0.005;
const BETA1: f64 = 0.9;
const BETA2: f64 = 0.999;

/// What to fit.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// Hidden `tanh` units; 0 for a logistic regression.
    pub hidden: usize,
    /// The L2 penalty on the weights (not the biases), against the mean log loss.
    pub l2: f64,
}

/// The logistic regression on all the features.
pub const LINEAR: Shape = Shape {
    hidden: 0,
    l2: 1e-3,
};

/// The network of 32 hidden units.
pub const MLP: Shape = Shape {
    hidden: 32,
    l2: 1e-4,
};

/// Each feature's mean and standard deviation on the training samples.
#[derive(Clone, Debug)]
pub struct Scale {
    mean: Vec<f64>,
    sd: Vec<f64>,
}

impl Scale {
    pub fn fit(xs: &[&[f64]]) -> Scale {
        let d = xs.first().map_or(0, |x| x.len());
        let n = xs.len().max(1) as f64;
        let mut mean = vec![0.0; d];
        for x in xs {
            for (m, v) in mean.iter_mut().zip(*x) {
                *m += v / n;
            }
        }
        let mut sd = vec![0.0; d];
        for x in xs {
            for ((s, v), m) in sd.iter_mut().zip(*x).zip(&mean) {
                *s += (v - m).powi(2) / n;
            }
        }
        for s in &mut sd {
            *s = s.sqrt();
        }
        Scale { mean, sd }
    }

    /// `x` standardised; a feature constant in training reads 0.
    pub fn apply(&self, x: &[f64]) -> Vec<f64> {
        x.iter()
            .zip(&self.mean)
            .zip(&self.sd)
            .map(|((v, m), s)| if *s > 1e-12 { (v - m) / s } else { 0.0 })
            .collect()
    }
}

/// A fitted model: its scale and parameters. With `h` hidden units over `d` features the
/// parameters are the hidden weights (`h × d`), the hidden biases, the output weights and
/// the output bias; with none, the `d` weights and the bias.
#[derive(Clone, Debug)]
pub struct Net {
    shape: Shape,
    d: usize,
    scale: Scale,
    p: Vec<f64>,
}

impl Net {
    fn len(shape: Shape, d: usize) -> usize {
        match shape.hidden {
            0 => d + 1,
            h => h * d + h + h + 1,
        }
    }

    /// Whether parameter `i` is a weight the L2 penalty applies to.
    fn penalised(&self, i: usize) -> bool {
        let (h, d) = (self.shape.hidden, self.d);
        match h {
            0 => i < d,
            h => i < h * d || (h * d + h..h * d + 2 * h).contains(&i),
        }
    }

    /// The chance of a win at standardised features `x`, and with `grad` the log loss's
    /// gradient for `label` added to it.
    fn forward(&self, x: &[f64], grad: Option<(&mut [f64], bool)>) -> f64 {
        let (h, d, p) = (self.shape.hidden, self.d, &self.p);
        if h == 0 {
            let z = x.iter().zip(p).map(|(x, w)| x * w).sum::<f64>() + p[d];
            let y = sigmoid(z);
            if let Some((g, label)) = grad {
                let e = y - f64::from(u8::from(label));
                for (gi, xi) in g.iter_mut().zip(x) {
                    *gi += e * xi;
                }
                g[d] += e;
            }
            return y;
        }
        let (w1, rest) = p.split_at(h * d);
        let (b1, rest) = rest.split_at(h);
        let (w2, b2) = rest.split_at(h);
        let a: Vec<f64> = (0..h)
            .map(|j| {
                let row = &w1[j * d..(j + 1) * d];
                (row.iter().zip(x).map(|(w, x)| w * x).sum::<f64>() + b1[j]).tanh()
            })
            .collect();
        let y = sigmoid(a.iter().zip(w2).map(|(a, w)| a * w).sum::<f64>() + b2[0]);
        if let Some((g, label)) = grad {
            let e = y - f64::from(u8::from(label));
            let (g1, rest) = g.split_at_mut(h * d);
            let (gb1, rest) = rest.split_at_mut(h);
            let (g2, gb2) = rest.split_at_mut(h);
            gb2[0] += e;
            for j in 0..h {
                g2[j] += e * a[j];
                let back = e * w2[j] * (1.0 - a[j] * a[j]);
                gb1[j] += back;
                for (gw, xi) in g1[j * d..(j + 1) * d].iter_mut().zip(x) {
                    *gw += back * xi;
                }
            }
        }
        y
    }

    /// The chance of a win at raw features `x`.
    pub fn predict(&self, x: &[f64]) -> f64 {
        self.forward(&self.scale.apply(x), None)
    }

    /// A model of `shape` fitted to `train`, from `seed`, keeping the parameters with the
    /// lowest log loss on `valid`; with no `valid`, those after the last pass.
    pub fn train(
        shape: Shape,
        train: &[(&[f64], bool)],
        valid: &[(&[f64], bool)],
        seed: u64,
    ) -> Net {
        let d = train.first().map_or(0, |(x, _)| x.len());
        let xs: Vec<&[f64]> = train.iter().map(|(x, _)| *x).collect();
        let scale = Scale::fit(&xs);
        let train: Vec<(Vec<f64>, bool)> =
            train.iter().map(|(x, y)| (scale.apply(x), *y)).collect();
        let valid: Vec<(Vec<f64>, bool)> =
            valid.iter().map(|(x, y)| (scale.apply(x), *y)).collect();
        let mut rng = SplitMix64::new(seed);
        let uniform = |rng: &mut SplitMix64| (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        let mut p = vec![0.0; Net::len(shape, d)];
        if shape.hidden > 0 {
            let h = shape.hidden;
            let (r1, r2) = ((6.0 / (d + h) as f64).sqrt(), (6.0 / (h + 1) as f64).sqrt());
            for w in &mut p[..h * d] {
                *w = r1 * (2.0 * uniform(&mut rng) - 1.0);
            }
            for w in &mut p[h * d + h..h * d + 2 * h] {
                *w = r2 * (2.0 * uniform(&mut rng) - 1.0);
            }
        }
        let mut net = Net { shape, d, scale, p };
        let mut order: Vec<usize> = (0..train.len()).collect();
        let (mut moment, mut spread) = (vec![0.0; net.p.len()], vec![0.0; net.p.len()]);
        let mut step = 0;
        let mut best = (f64::INFINITY, net.p.clone());
        let mut waited = 0;
        for _ in 0..EPOCHS {
            for i in (1..order.len()).rev() {
                order.swap(i, rng.below(i as u64 + 1) as usize);
            }
            for batch in order.chunks(BATCH) {
                let mut g = vec![0.0; net.p.len()];
                for &i in batch {
                    net.forward(&train[i].0, Some((&mut g, train[i].1)));
                }
                step += 1;
                let n = batch.len() as f64;
                let (c1, c2) = (1.0 - BETA1.powi(step), 1.0 - BETA2.powi(step));
                for i in 0..net.p.len() {
                    let mut gi = g[i] / n;
                    if net.penalised(i) {
                        gi += shape.l2 * net.p[i];
                    }
                    moment[i] = BETA1 * moment[i] + (1.0 - BETA1) * gi;
                    spread[i] = BETA2 * spread[i] + (1.0 - BETA2) * gi * gi;
                    net.p[i] -= RATE * (moment[i] / c1) / ((spread[i] / c2).sqrt() + 1e-8);
                }
            }
            if valid.is_empty() {
                continue;
            }
            let loss = valid
                .iter()
                .map(|(x, y)| log_loss(net.forward(x, None), *y))
                .sum::<f64>()
                / valid.len() as f64;
            if loss < best.0 {
                best = (loss, net.p.clone());
                waited = 0;
            } else {
                waited += 1;
                if waited >= PATIENCE {
                    break;
                }
            }
        }
        if !valid.is_empty() {
            net.p = best.1;
        }
        net
    }
}

/// The log loss of predicting `p` for `label`, with `p` kept off 0 and 1.
pub fn log_loss(p: f64, label: bool) -> f64 {
    let p = p.clamp(1e-6, 1.0 - 1e-6);
    -if label { p.ln() } else { (1.0 - p).ln() }
}

/// The log-odds of `p`, with `p` kept off 0 and 1: a search's mean reward is exactly 1 when
/// every rollout won before the turn discount applied.
pub fn logit(p: f64) -> f64 {
    let p = p.clamp(1e-3, 1.0 - 1e-3);
    (p / (1.0 - p)).ln()
}
