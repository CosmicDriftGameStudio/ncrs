use crate::error::IndexError;
use crate::index::Index;
use crate::model::{SenderAlias, SenderAliasKind, SenderAliasState, SenderAliases};

impl Index {
    /// Ordered by sender name, kind, value.
    pub fn sender_aliases(&self) -> Result<Vec<SenderAlias>, IndexError> {
        let mut statement = self.connection().prepare(
            "SELECT s.name, a.kind, a.value, a.state
             FROM sender_alias a JOIN sender s ON s.id = a.sender_id
             ORDER BY s.name, a.kind, a.value",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(sender, kind, value, state)| {
                Ok(SenderAlias {
                    sender,
                    kind: SenderAliasKind::parse(&kind)?,
                    value,
                    state: SenderAliasState::parse(&state)?,
                })
            })
            .collect()
    }

    pub fn confirmed_sender_aliases(&self) -> Result<SenderAliases, IndexError> {
        let all = self.sender_aliases()?;
        let confirmed = all
            .iter()
            .filter(|alias| alias.state == SenderAliasState::Confirmed)
            .map(|alias| (alias.kind, alias.value.as_str(), alias.sender.as_str()));
        Ok(SenderAliases::from_confirmed(confirmed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{name_id, NameTable};
    use crate::test_support::encrypted_index;

    fn store(index: &Index, sender: &str, kind: &str, value: &str, state: &str) {
        let sender_id = name_id(index.connection(), NameTable::Sender, sender).unwrap();
        index
            .connection()
            .execute(
                "INSERT INTO sender_alias (sender_id, kind, value, state) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![sender_id, kind, value, state],
            )
            .unwrap();
    }

    #[test]
    fn a_confirmed_alias_resolves_the_sender_in_any_spelling() {
        let (_dir, index) = encrypted_index();
        store(
            &index,
            "A GmbH",
            "iban",
            "DE89370400440532013000",
            "confirmed",
        );
        store(
            &index,
            "Telekom",
            "name",
            "telekom deutschland gmbh",
            "confirmed",
        );

        let aliases = index.confirmed_sender_aliases().unwrap();

        assert_eq!(
            aliases.sender_for(SenderAliasKind::Iban, "de89 3704 0044 0532 0130 00"),
            Some("A GmbH")
        );
        assert_eq!(
            aliases.canonical_sender("telekom  deutschland gmbh"),
            "Telekom"
        );
        assert_eq!(aliases.canonical_sender("Other"), "Other");
    }

    #[test]
    fn an_offered_or_discarded_alias_resolves_nothing() {
        let (_dir, index) = encrypted_index();
        store(&index, "A GmbH", "domain", "a.example", "offered");
        store(
            &index,
            "Telekom",
            "name",
            "telekom deutschland gmbh",
            "discarded",
        );

        let aliases = index.confirmed_sender_aliases().unwrap();

        assert_eq!(index.sender_aliases().unwrap().len(), 2);
        assert_eq!(aliases, SenderAliases::default());
    }
}
