# Review of the word list `groceries-v1`

`groceries-v1.tsv` is the word list of the memory contest's shopping list (content version 1). Every
player gets the same concept ids and sees them in their own language, so each language column needs a
review by a native speaker before the contest ships. German is reviewed by the owner.

## Status

| Language | Status | Reviewer | Date |
|---|---|---|---|
| English (en) | draft (2026-10-24, Claude), review open | | |
| German (de) | draft (2026-10-24, Claude), review open (owner) | | |
| French (fr) | draft (2026-10-24, Claude), review open | | |
| Japanese (ja) | draft (2026-10-24, Claude), review open | | |
| Chinese (zh) | draft (2026-10-24, Claude), review open | | |

When a review is done, put the reviewer and the date into the table and replace "review open" with
"reviewed".

## The file

One concept per line, tab-separated, sorted by `id`, after a header line:

| Column | Content |
|---|---|
| `id` | Stable number, from 1. Never reused or renumbered. |
| `key` | English slug, for reviews only. |
| `category` | `fruit`, `vegetables`, `dairy`, `bakery`, `meat_fish`, `pantry`, `drinks`, `frozen_snacks`, `household`, `personal_care` |
| `group` | Exclusivity group: at most one concept of a group goes into a list. Empty if none. |
| `en` `de` `fr` `ja` `zh` | `display;alt;alt…`: the first form is shown, all forms are accepted. |

The draft has 140 concepts, 8 to 24 per category. The spec's target of 200 was not reached: a concept
must be sold in ordinary supermarkets in Germany, France, the UK/US, Japan and China, and its display form
must fit (Latin scripts at most 14 characters, Japanese at most 5, Chinese at most 3). Broccoli,
pineapple, blueberries, croissants and toilet paper fail the Japanese limit, pork the German one
(Schweinefleisch). More concepts can come with a reviewed version 2.

## How an answer is matched

Answers and forms are compared after normalization: case, full-width letters, katakana versus hiragana and
Latin accents do not matter, and spaces, hyphens and apostrophes are dropped. A Latin form (pinyin too) of
at least 5 letters, or a kana form of at least 4, also accepts one typo. Chinese characters and kanji must
be exact; that is why every Japanese concept lists its reading in kana and every Chinese concept its
pinyin. There is no autocomplete, so what a person types is what counts.

## Checklist for each language

For every row:

1. **The word a shopping list would use.** Is it what people in your country write on a shopping list?
2. **The display form.** Is the first form the most natural one to show? It must be short (see above).
   The draft shows single fruits and vegetables in the singular, as in the spec's example rows ("apple",
   "Apfel", "pomme"), and things a shopping list only names in the plural in the plural ("eggs",
   "Eier", "œufs"). Change this where it reads oddly.
3. **Accepted alternatives.** Are the usual other ways to write it listed?
   - plurals (en, de, fr);
   - Japanese: kanji, hiragana and katakana, and the reading in kana;
   - Chinese: simplified and traditional characters, and toneless pinyin without spaces (`pingguo`).
4. **Nothing ambiguous.** No form may also mean another concept of the list, and no form should be a
   common word for something else on a shopping list.
5. **Sold everywhere.** Is the concept an ordinary supermarket item in your country? Flag the ones that are
   not.

The tests (`tests/words.rs`) check the mechanical rules: every language filled, unique forms, display
lengths, readings and pinyin present, and that no forgiven typo can reach another concept unless both are in
one group. Run `cargo test -p locora-competition` after every change.

## Points for the reviewers

- **en:** British spelling and words, as in the app ("courgette", "aubergine", "crisps", "sweets", "mince",
  "nappies", "plasters", "bin bags"); the American words are accepted alternatives. Is "bell pepper" right as
  the display form, or "pepper"? "detergent" or "washing powder"?
- **de:** "Salat" for lettuce, "Sellerie" (celeriac or celery?), "Müsli" for breakfast cereal, "Deo",
  "Würstchen" for sausages. "Kaese" and "Oel" are listed because they are too short for typo tolerance.
- **fr:** "lardons" for bacon, "donut" for doughnut, "piment" for chilli, "chips" for crisps. Typed without
  accents or with "oe" for "œ" is always accepted.
- **ja:** "長ねぎ" for leek, "コーン" for sweetcorn (とうもろこし is too long to display), "ポテチ" for
  crisps, "制汗剤" for deodorant, "フロス" for dental floss (or 糸ようじ?), "飴" for sweets. Is "お米" right
  as the display form for rice, and "梨" or "なし" for pear?
- **zh:** "哈密瓜" for melon, "意面" for pasta (意大利面 has four characters), "止汗露" for deodorant, "餐包" for
  bread rolls, "玛芬" for muffin, "黄芥末" for mustard. "梨子" is accepted for pear but its pinyin "lizi" is
  not, because it is the pinyin of plum (李子).

## Groups

Groups keep near-duplicates out of the same list, and pairs whose forms are one typo apart:

| Group | Concepts | Why |
|---|---|---|
| `orange` | orange, mandarin | similar fruit |
| `lemon` | lemon, lime | similar fruit |
| `melon` | watermelon, melon | similar fruit |
| `grapes` | grapes, raisins | French "raisin" means grape |
| `bread` | bread, sliced bread, bread rolls | similar bread |
| `oil` | oil, olive oil | similar oil |
| `water` | water, fizzy water | similar drink |
| `tea` | tea, green tea | similar drink |
| `oeuf_boeuf` | eggs, beef | French "œuf" and "bœuf" are one letter apart |
| `donatsu_pinatsu` | doughnut, peanuts | Japanese ドーナツ and ピーナツ are one letter apart |
| `mangguo_tangguo` | mango, sweets | pinyin "mangguo" and "tangguo" are one letter apart |

An answer that is exactly a form of another concept counts as that concept, never as a typo: "ドーナツ"
is not a misspelt "ピーナツ".

## Freeze

`groceries_v1_is_frozen` pins the SHA-256 of the file. Until the first release that ships the contest,
a reviewed correction changes the file, the pinned hash and `tests/golden/list.json` (the lists of a fixed
seed) in the same commit. From that release on, version 1 never changes: corrections go into
`groceries-v2.tsv` with `CONTENT = 2`, next to version 1.
