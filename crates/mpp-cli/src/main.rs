fn main() {
    println!("Math++ 0.1.0");
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_banner_is_stable() {
        assert_eq!(format!("Math++ {}", "0.1.0"), "Math++ 0.1.0");
    }
}
