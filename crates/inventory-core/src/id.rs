/// Generate a new ULID string (26 chars, Crockford base32, lexicographically sortable).
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_26_char_ulids_and_unique() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.len(), 26);
        assert_ne!(a, b);
        assert!(ulid::Ulid::from_string(&a).is_ok());
    }
}
