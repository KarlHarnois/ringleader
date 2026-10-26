pub struct GhQuery {
    branches: Vec<String>,
}

impl GhQuery {
    const CANDIDATES: &'static str = "first: 10, orderBy: {field: CREATED_AT, direction: DESC}";
    const FIELDS: &'static str = "number url state isDraft createdAt headRepositoryOwner { login }";
    const REVIEW_FIELDS: &'static str = "nodes { state }";
    const REVIEW_LIMIT: usize = 100;

    pub fn new(branches: &[String]) -> Self {
        Self {
            branches: branches.to_vec(),
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec!["-f".to_string(), format!("query={}", self.text())];
        for (alias, branch) in self.aliased() {
            arguments.extend(["-f".to_string(), format!("{alias}={branch}")]);
        }
        arguments
    }

    pub fn aliased(&self) -> impl Iterator<Item = (String, &String)> {
        self.branches
            .iter()
            .enumerate()
            .map(|(index, branch)| (format!("branch{index}"), branch))
    }

    fn text(&self) -> String {
        let variables: String = self
            .aliased()
            .map(|(alias, _)| format!(", ${alias}: String!"))
            .collect();
        let fields: String = self
            .aliased()
            .map(|(alias, _)| Self::field(&alias))
            .collect();
        format!(
            "query($owner: String!, $name: String!{variables}) \
             {{ viewer {{ login }} \
             repository(owner: $owner, name: $name) {{ owner {{ login }}{fields} }} }}"
        )
    }

    fn field(alias: &str) -> String {
        format!(
            " {alias}: pullRequests(headRefName: ${alias}, {}) {{ nodes {{ {} {} }} }}",
            Self::CANDIDATES,
            Self::FIELDS,
            Self::review_fields()
        )
    }

    fn review_fields() -> String {
        format!(
            "latestOpinionatedReviews(first: {}, writersOnly: true) {{ {} }}",
            Self::REVIEW_LIMIT,
            Self::REVIEW_FIELDS
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_holds_one_aliased_field_per_branch() {
        let text = GhQuery::new(&["fix-auth".to_string(), "dark-mode".to_string()]).text();

        assert!(text.contains("$branch0: String!, $branch1: String!"));
        assert!(text.contains("branch1: pullRequests(headRefName: $branch1,"));
        assert!(text.contains("viewer { login }"));
        assert!(text.contains("headRepositoryOwner { login }"));
    }

    #[test]
    fn the_query_asks_for_the_url_of_every_pull_request() {
        let arguments = GhQuery::new(&["fix-auth".to_string()]).arguments();
        let query = arguments
            .iter()
            .find(|argument| argument.starts_with("query="))
            .unwrap();

        assert!(query.contains(" url "));
    }

    #[test]
    fn the_query_asks_for_the_review_of_every_pull_request() {
        let arguments = GhQuery::new(&["fix-auth".to_string()]).arguments();
        let query = arguments
            .iter()
            .find(|argument| argument.starts_with("query="))
            .unwrap();
        let review = format!(
            "latestOpinionatedReviews(first: {}, writersOnly: true) {{ nodes {{ state }} }}",
            GhQuery::REVIEW_LIMIT
        );

        assert!(query.contains(&review));
    }

    #[test]
    fn the_review_limit_is_the_github_connection_maximum() {
        assert_eq!(GhQuery::REVIEW_LIMIT, 100);
    }
}
