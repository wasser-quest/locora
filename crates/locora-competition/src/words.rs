//! The word list of the shopping list, `groceries-v1` (content version 1).
//!
//! `data/groceries-v1.tsv` holds one grocery concept per line: a stable id, an
//! English key for reviews, one of ten categories, an optional exclusivity
//! group, and per language the display form followed by accepted alternatives
//! (`display;alt;alt…`). Everyone gets the same concept ids; each person sees
//! them in their own language. The file is compiled in and parsed once.
//!
//! The list is frozen with the release that ships the contest: corrections after
//! that go into `groceries-v2.tsv` with content version 2 (see `data/REVIEW.md`).

use crate::Lang;
use crate::matcher::normalize;
use std::collections::HashMap;
use std::sync::OnceLock;

const GROCERIES_V1: &str = include_str!("../data/groceries-v1.tsv");
const HEADER: &str = "id\tkey\tcategory\tgroup\ten\tde\tfr\tja\tzh";

/// The ten categories. A list takes at most `ceil(items / 10)` from each, so a
/// list of ten has one of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Fruit,
    Vegetables,
    Dairy,
    Bakery,
    MeatFish,
    Pantry,
    Drinks,
    FrozenSnacks,
    Household,
    PersonalCare,
}

impl Category {
    pub const ALL: [Category; 10] = [
        Category::Fruit,
        Category::Vegetables,
        Category::Dairy,
        Category::Bakery,
        Category::MeatFish,
        Category::Pantry,
        Category::Drinks,
        Category::FrozenSnacks,
        Category::Household,
        Category::PersonalCare,
    ];

    /// The name in the word list.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Fruit => "fruit",
            Category::Vegetables => "vegetables",
            Category::Dairy => "dairy",
            Category::Bakery => "bakery",
            Category::MeatFish => "meat_fish",
            Category::Pantry => "pantry",
            Category::Drinks => "drinks",
            Category::FrozenSnacks => "frozen_snacks",
            Category::Household => "household",
            Category::PersonalCare => "personal_care",
        }
    }

    pub fn parse(text: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.as_str() == text)
    }
}

/// One grocery concept with its forms in the five languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Concept {
    pub id: u16,
    /// English slug, for reviews only.
    pub key: &'static str,
    pub category: Category,
    /// At most one concept of a group goes into a list (near-duplicates such as
    /// lemon and lime, or words one typo apart).
    pub group: Option<&'static str>,
    forms: [Vec<&'static str>; 5],
    normalized: [Vec<String>; 5],
}

impl Concept {
    /// The form shown in `lang`.
    pub fn display(&self, lang: Lang) -> &'static str {
        self.forms[lang as usize][0]
    }

    /// Every accepted form in `lang`, the display form first.
    pub fn forms(&self, lang: Lang) -> &[&'static str] {
        &self.forms[lang as usize]
    }

    /// The accepted forms in `lang` after [`normalize`], in the same order.
    pub fn normalized(&self, lang: Lang) -> &[String] {
        &self.normalized[lang as usize]
    }
}

/// A parsed word list.
#[derive(Debug)]
pub struct Pool {
    concepts: Vec<Concept>,
    /// Per language: normalized form → concept id.
    index: [HashMap<String, u16>; 5],
}

impl Pool {
    /// All concepts, sorted by id.
    pub fn concepts(&self) -> &[Concept] {
        &self.concepts
    }

    pub fn get(&self, id: u16) -> Option<&Concept> {
        let i = self.concepts.binary_search_by_key(&id, |c| c.id).ok()?;
        Some(&self.concepts[i])
    }

    /// The concept that has `normalized` among its forms in `lang`.
    pub fn find(&self, lang: Lang, normalized: &str) -> Option<u16> {
        self.index[lang as usize].get(normalized).copied()
    }

    /// Parses the TSV: the header, then one concept per line with ascending ids.
    fn parse(text: &'static str) -> Result<Pool, String> {
        let mut lines = text.lines().enumerate();
        if lines.next().map(|(_, l)| l) != Some(HEADER) {
            return Err("Die Kopfzeile der Wortliste fehlt oder ist falsch".into());
        }
        let mut concepts: Vec<Concept> = Vec::new();
        for (n, line) in lines {
            let row = n + 1;
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&'static str> = line.split('\t').collect();
            let [id, key, category, group, en, de, fr, ja, zh] = fields[..] else {
                return Err(format!(
                    "Zeile {row}: erwartet 9 Spalten, nicht {}",
                    fields.len()
                ));
            };
            let id: u16 = id
                .parse()
                .map_err(|_| format!("Zeile {row}: „{id}“ ist keine Id"))?;
            if concepts.last().is_some_and(|c| c.id >= id) || id == 0 {
                return Err(format!("Zeile {row}: Id {id} ist nicht aufsteigend"));
            }
            let category = Category::parse(category)
                .ok_or_else(|| format!("Zeile {row}: unbekannte Kategorie „{category}“"))?;
            let mut forms: [Vec<&'static str>; 5] = Default::default();
            for (slot, cell) in forms.iter_mut().zip([en, de, fr, ja, zh]) {
                *slot = cell.split(';').collect();
                if slot.iter().any(|f| f.trim().is_empty()) {
                    return Err(format!("Zeile {row}: leere Form in „{cell}“"));
                }
            }
            let normalized = Lang::ALL.map(|lang| {
                forms[lang as usize]
                    .iter()
                    .map(|f| normalize(lang, f))
                    .collect()
            });
            concepts.push(Concept {
                id,
                key,
                category,
                group: (!group.is_empty()).then_some(group),
                forms,
                normalized,
            });
        }
        let mut index: [HashMap<String, u16>; 5] = Default::default();
        for concept in &concepts {
            for lang in Lang::ALL {
                for form in concept.normalized(lang) {
                    // A form two concepts share stays with the first; the tests forbid it anyway.
                    index[lang as usize]
                        .entry(form.clone())
                        .or_insert(concept.id);
                }
            }
        }
        Ok(Pool { concepts, index })
    }
}

/// The word list `groceries-v1`.
pub fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        Pool::parse(GROCERIES_V1).expect("groceries-v1.tsv wird in den Tests geprüft")
    })
}

/// The display form of a concept in `lang`; `None` for an id the list does not have.
pub fn display(id: u16, lang: Lang) -> Option<&'static str> {
    pool().get(id).map(|c| c.display(lang))
}

/// The category of a concept; `None` for an id the list does not have.
pub fn category(id: u16) -> Option<Category> {
    pool().get(id).map(|c| c.category)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_parses_and_looks_up() {
        let pool = pool();
        let apple = pool.concepts().iter().find(|c| c.key == "apple").unwrap();
        assert_eq!(apple.display(Lang::De), "Apfel");
        assert_eq!(apple.forms(Lang::Ja), ["りんご", "リンゴ", "林檎"]);
        assert_eq!(apple.normalized(Lang::Ja)[1], "りんご");
        assert_eq!(display(apple.id, Lang::Zh), Some("苹果"));
        assert_eq!(category(apple.id), Some(Category::Fruit));
        assert_eq!(pool.find(Lang::De, "apfel"), Some(apple.id));
        assert_eq!(pool.find(Lang::En, "apfel"), None);
        assert_eq!(display(0, Lang::En), None);
        assert_eq!(category(u16::MAX), None);
        for c in Category::ALL {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
    }

    #[test]
    fn broken_lists_are_refused() {
        let ok = "1\tapple\tfruit\t\ta\tb\tc\td\te";
        assert!(Pool::parse(Box::leak(format!("{HEADER}\n{ok}\n").into_boxed_str())).is_ok());
        // Windows line endings are read the same.
        assert!(Pool::parse(Box::leak(format!("{HEADER}\r\n{ok}\r\n").into_boxed_str())).is_ok());
        for bad in [
            format!("{ok}\n"),
            format!("{HEADER}\n1\tapple\tfruit\t\ta\tb\tc\td\n"),
            format!("{HEADER}\nx\tapple\tfruit\t\ta\tb\tc\td\te\n"),
            format!("{HEADER}\n0\tapple\tfruit\t\ta\tb\tc\td\te\n"),
            format!("{HEADER}\n{ok}\n{ok}\n"),
            format!("{HEADER}\n1\tapple\tfruits\t\ta\tb\tc\td\te\n"),
            format!("{HEADER}\n1\tapple\tfruit\t\ta;\tb\tc\td\te\n"),
        ] {
            assert!(
                Pool::parse(Box::leak(bad.clone().into_boxed_str())).is_err(),
                "{bad}"
            );
        }
    }
}
