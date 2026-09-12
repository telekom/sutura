//! The generated interface description is the governed tool surface, in full and nothing else.

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use sutura_http::capability::{capability_of, governed};
    use sutura_http::openapi::document;

    fn documented_operations() -> Vec<(Method, String)> {
        let document = document();
        let mut found = Vec::new();
        for (path, item) in &document.paths.paths {
            for (method, present) in [
                (Method::GET, item.get.is_some()),
                (Method::PUT, item.put.is_some()),
                (Method::POST, item.post.is_some()),
                (Method::DELETE, item.delete.is_some()),
                (Method::OPTIONS, item.options.is_some()),
                (Method::HEAD, item.head.is_some()),
                (Method::PATCH, item.patch.is_some()),
                (Method::TRACE, item.trace.is_some()),
            ] {
                if present {
                    found.push((method, path.clone()));
                }
            }
        }
        found
    }

    #[test]
    fn generated_document_describes_exactly_the_governed_operations() {
        let documented = documented_operations();
        for (method, path) in &documented {
            assert!(capability_of(method, path).is_some(), "{method} {path} is ungoverned");
        }
        assert_eq!(documented.len(), governed().len());
        assert!(!documented.iter().any(|(_, path)| path == "/health"));
    }
}
