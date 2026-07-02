// Real output of `smelt build` for input.ts (runtime prelude omitted).
fn celsius(fahrenheit: f64) -> f64 {
    let _smelt_tmp_1: f64 = fahrenheit.clone() - 32.0;
    let _smelt_tmp_2: f64 = _smelt_tmp_1 * 5.0;
    let _smelt_tmp_3: f64 = _smelt_tmp_2 / 9.0;
    return _smelt_tmp_3;
}
fn fibonacci(n: f64) -> f64 {
    let mut next: f64;
    let mut _smelt_tmp_5: bool;
    let mut _smelt_tmp_6: f64;
    let mut _smelt_tmp_7: f64;
    let mut a: f64 = 0.0;
    let mut b: f64 = 1.0;
    let mut i: f64 = 0.0;
    loop {
    _smelt_tmp_5 = i.clone() < n.clone();
    if !(_smelt_tmp_5) { break; }
    _smelt_tmp_6 = a + b.clone();
    next = _smelt_tmp_6;
    a = b;
    b = next;
    _smelt_tmp_7 = i + 1.0;
    i = _smelt_tmp_7;
    }
    return a;
}
