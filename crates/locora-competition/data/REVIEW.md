# Review of the word list `groceries-v1`

`groceries-v1.tsv` is the word list of the memory contest's shopping list (content version 1). Every
player gets the same concept ids and sees them in their own language, so each language column needs a
review by a native speaker before the contest ships. German is reviewed by the owner.

## Status

| Language | Status | Reviewer | Date |
|---|---|---|---|
| English (en) | AI review done (2026-10-24, Claude), changes below; native review open | Claude (AI) | 2026-10-24 |
| German (de) | draft (2026-10-24, Claude), checked by Claude (2026-10-24), review open (owner) | | |
| French (fr) | draft (2026-10-24, Claude), checked by Claude (2026-10-24), native review open | | |
| Japanese (ja) | draft (2026-10-24, Claude), AI review (2026-10-24, Claude) done, native review open | | |
| Chinese (zh) | AI review done (changes below); a human native speaker should confirm before the freeze | Claude (AI) | 2026-10-24 |

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
blueberries, croissants and toilet paper fail the Japanese limit, pork the German one
(Schweinefleisch); pineapple would fit (Japanese パイン). More concepts can come with a reviewed version 2.

## How an answer is matched

Answers and forms are compared after normalization: case, full-width letters, half-width katakana,
katakana versus hiragana and Latin accents do not matter, and spaces, hyphens and apostrophes are dropped.
A kana typed apart from its sound mark (か and ゛) is joined with it. In English, German and French one
leading article is dropped ("an apple", "die Milch", "l’huile", "de la farine"); in German "ae", "oe" and
"ue" count as "ä", "ö" and "ü"; in Chinese a quantity before or after the item is dropped ("一瓶牛奶",
"鸡蛋两盒", "牛奶x2"). A Latin form (pinyin too) of at least 5 letters, or a kana form of at least 4, also
accepts one typo. Chinese characters and kanji must be exact; that is why every Japanese concept lists its
reading in kana and every Chinese concept its pinyin. An answer that is exactly a form of another concept
counts as that concept, never as a typo. There is no autocomplete, so what a person types is what counts.

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

## English (en)

Reviewed on 2026-10-24 by Claude (AI), reading the list as a native speaker would. A scratch test scored
these answers through `score`: every form, its plural or singular, the form with "a", "an", "the" and
"some" in front, and about 130 typed answers. A human native speaker still has to confirm the review.

**Words.** All 140 concepts are ordinary supermarket items in the UK and the US. Persimmons ("sharon
fruit" in the UK) and figs are seasonal, but supermarkets sell them. The spelling is British, as in the
app, and the American words are accepted. Nothing is offensive. "toast bread" is not English, so it is
dropped. "deo" is German usage but harmless, so it stays. Some accepted forms mean something else in one
country: "chips" (UK: fries), "squash" (UK: a fruit drink), "bandages" (UK: wraps) and "biscuits" (US: a
scone-like bread). This does no harm because no other concept uses them.

**Open points.** "bell pepper" stays the display form, because "pepper" on its own reads as the spice.
"pepper", "peppers" and "red peppers" are accepted. "detergent" stays: people in both countries
understand it, and it covers both powder and liquid.

**Display forms.** An English shopping list puts produce, and things bought several at a time, in the
plural ("apples, carrots, onions"). Next to "mushrooms" and "grapes", a singular reads like a single
piece. The plural goes first for apples, bananas, oranges, mandarins, lemons, limes, strawberries, pears,
peaches, cherries, kiwis, mangoes, avocados, plums, persimmons, figs, potatoes, tomatoes, carrots,
onions, bell peppers, courgettes, leeks, sweet potatoes, chillies, doughnuts, waffles, muffins and light
bulbs. Things bought one at a time stay singular: watermelon, melon, coconut, cucumber, lettuce, cabbage,
pumpkin, aubergine, cake, sponge, lighter, toothbrush and razor. "sesame" becomes "sesame seeds".

**Accepted forms added.** Before the review each of these answers was wrong, or counted only as a typo:

- Plurals that short forms cannot reach with a typo: jams, beers, wines, bulbs, lightbulbs, bun, loaves.
- UK words: satsumas, kiwi fruit, red/green/sweet peppers, chilli peppers, butternut squash, single
  cream, minced beef, beef mince, tinned tuna, still water, laundry liquid, pegs, bubble gum, sliced
  loaf.
- US words: string beans, heavy cream, sweetened condensed milk, canned tuna, seltzer, club soda,
  candies, sunblock, antiperspirant, facial tissues, sandwich bread.

No new form is a form of another concept. None is one typo from another concept's form, except "candies"
and "candles", which the group `sweets` covers.

**Groups.** `sweets` (mango, sweets, candles, and sugar from the Chinese review) replaces
`mangguo_tangguo`. Without it, "candies" counts as a typo for candles and is wrong for sweets. The new
group `wine` (wine, champagne) follows tea and green tea: champagne is accepted as "sparkling wine",
which is a kind of wine. The group `flour` (flour, flowers) was optional; it is in and covers the list's
one pair of homophones.

**Matcher.** English answers drop one leading "a", "an", "the" or "some" (a new decision on §8.7).
Without that rule, "an apple", "an onion", "a pear", "a lime" and "a loaf" are wrong, while "a banana"
counts only as a typo. Common misspellings one edit away are accepted: "potatos", "zuchini", "mackeral",
"mayonaise", "deoderant", "marshmellows". "cinammon" is two edits away and is not accepted, as the spec
says. Known and accepted under §8.7: a word for a grocery outside the list that is one letter from a
listed form counts as that item ("peas" for pears, "chops" for crisps through "chips", "paste" for
pasta).

**Freeze.** These changes update `groceries-v1.tsv` and `FROZEN` in `tests/words.rs` in one commit (D15).
The golden lists of `tests/golden/list.json` were generated again and stay the same: no new group meets
on their seeds.

## German (de)

Claude checked this column against the checklist on 2026-10-24. The spec says the owner reviews German,
so the status stays "review open (owner)" until the owner signs off.

**Items.** All 140 concepts are things people buy in ordinary supermarkets or drugstores in Germany,
Austria and Switzerland. Kaki, Marshmallows, Nagelknipser and Sesam are borderline but are sold
everywhere, so they stay. Nothing in the column is offensive. "Mohrrübe" comes from "Möhre", not from
"Mohr", and is standard German.

**Answers to the points above.**

- "Salat" is the word a German shopping list uses for lettuce. Keep it.
- "Sellerie" often means celeriac in Germany. The precise word, "Staudensellerie", has 15 letters and
  does not fit the display. Keep "Sellerie" as the display form, with "Staudensellerie" and
  "Stangensellerie" accepted.
- "Müsli", "Deo" and "Würstchen" are the everyday words. Keep them.
- Keep "Kaese" and "Oel". "Kase" and "Ol" already match through the umlaut folding.

**Display forms.** Nobody buys or lists strawberries or cherries one at a time, so show the plurals
"Erdbeeren" and "Kirschen". The other singular fruit and vegetables read naturally.

**Changes.** Each row below gives the new `de` cell. The German changes leave ids, keys and groups alone;
with the other reviews they change the pinned hash in `groceries_v1_is_frozen`, and
`tests/golden/list.json` stays the same.

| id | key | de |
|---|---|---|
| 7 | strawberry | `Erdbeeren;Erdbeere` |
| 11 | cherry | `Kirschen;Kirsche` |
| 21 | potato | `Kartoffel;Kartoffeln;Erdäpfel` |
| 22 | tomato | `Tomate;Tomaten;Paradeiser` |
| 23 | carrot | `Karotte;Karotten;Möhre;Möhren;Mohrrübe;Mohrrüben;Rüebli;Gelbe Rüben` |
| 27 | lettuce | `Salat;Kopfsalat;Blattsalat;Eisbergsalat` |
| 29 | cabbage | `Kohl;Weißkohl;Kohlkopf;Weißkraut` |
| 33 | courgette | `Zucchini;Zucchinis;Zucchetti` |
| 34 | aubergine | `Aubergine;Auberginen;Melanzani` |
| 40 | green_beans | `grüne Bohnen;Bohnen;Brechbohnen;Fisolen` |
| 44 | chilli | `Chili;Chilis;Peperoni;Chilischote;Pfefferoni` |
| 51 | cream | `Sahne;Schlagsahne;Rahm;Schlagobers;Obers` |
| 55 | bread_rolls | `Brötchen;Semmeln;Semmel;Schrippen;Weckerl;Weggli` |
| 61 | cereal | `Müsli;Cornflakes;Frühstücksflocken;Cerealien` |
| 63 | chicken | `Hähnchen;Hühnchen;Hähnchenfleisch;Huhn;Hühnerfleisch;Hähnchenbrust;Hendl;Poulet` |
| 65 | mince | `Hackfleisch;Hack;Gehacktes;Faschiertes` |
| 67 | tuna | `Thunfisch;Tunfisch` |
| 69 | sausages | `Würstchen;Wurst;Würste;Würstel` |
| 76 | sardines | `Sardinen;Sardine;Ölsardinen` |
| 78 | pasta | `Nudeln;Pasta;Spaghetti;Spagetti` |
| 89 | mayonnaise | `Mayonnaise;Mayo;Majonäse` |
| 97 | water | `Wasser;Mineralwasser;Tafelwasser;stilles Wasser` |
| 99 | juice | `Saft;Orangensaft;Apfelsaft;Fruchtsaft;O-Saft` |
| 102 | champagne | `Sekt;Champagner;Schaumwein;Prosecco` |
| 107 | pizza | `Pizza;Tiefkühlpizza;TK-Pizza` |
| 114 | raisins | `Rosinen;Rosine;Sultaninen` |
| 117 | marshmallows | `Marshmallows;Marshmallow;Mäusespeck` |
| 118 | detergent | `Waschmittel;Waschpulver` |
| 122 | light_bulb | `Glühbirne;Glühbirnen;Glühlampe;Glühlampen` |
| 126 | matches | `Streichhölzer;Streichholz;Zündhölzer;Zündholz` |
| 128 | clothes_pegs | `Wäscheklammern;Wäscheklammer;Wäschekluppen` |
| 140 | nail_clippers | `Nagelknipser;Nagelzwicker` |

The matcher now reads "ae/oe/ue" as umlauts (see below), so `Fruehstuecksflocken` (61),
`Fruehstuecksspeck` (71), `Muellsaecke` and `Muelltueten` (121) and `Zuendhoelzer` (126) match without
being listed. Every new form was checked against the rules of `tests/words.rs`. No form is shared with
another concept, no new form is one typo from another concept's form, and both display forms fit within
14 characters.

**Not added.**

- "Zuckerl" (the Austrian word for sweets) is one typo from "Zucker" (sugar).
- "Tempos" is a brand.
- "Birne" stays with the pear only, although people also say it for a light bulb.
- "Nagelschere" (nail scissors) is a different tool.

**Ambiguous but kept.**

- "Peperoni" is a hot pepper in Germany and Austria but a bell pepper in Switzerland. The displays are
  "Chili" and "Paprika", so "Peperoni" stays with chilli.
- In Germany "Mineralwasser" is sparkling unless it says otherwise. Water and fizzy water share a group,
  so they never appear in the same list.

**Matcher.** Checked with a probe that calls the crate.

- **Articles.** An answer that started with an article was wrong: "die Milch", "ein Brot", "eine Gurke".
  The matcher now has a German rule that drops one leading article (der, die, das, den, dem, des, ein,
  eine, einen, einem, einer, eines) when a word follows it. This changes spec §8.7. No German form starts
  with an article, so no form is affected.
- **ae/oe/ue.** "Aepfel", "Broetchen" and "Suesskartoffel" were marked "right (typo accepted)", and that
  used up the one allowed typo, so "Broetchn" was wrong. The five forms with two umlauts were not
  accepted at all ("Muellsaecke", "Zuendhoelzer"). The matcher now has a German rule that maps "ae",
  "oe", "ue" to "a", "o", "u" after step 5, for answers and forms alike. Apart from forms with umlauts,
  it changes only "Kaese", "Oel" and "Feuerzeug", and it creates no clashes.
- **Plurals and singulars.** Each one is listed, or is one typo from a form long enough to allow a typo
  (Bohne, Krabbe, Torten, Kaugummis, Rasierklinge, Mülltüte, Blume).
- **Spelling variants.** "ß" typed as "ss" ("Weisskohl", "Süsskartoffel") matches exactly. Hyphens and
  spaces in compounds ("Tiefkühl-Pizza") do not matter.
- **Confusable words.** These forms belong to different concepts and are too short for a typo, so each
  matches only its own concept: "Ei" and "Eis", "Reis" and "Eis", "Eier" and "Bier". "Lauch" is never
  accepted as "Lachs".
- **Counts.** A leading count such as "2 Bananen" passes only as a typo, and only for long words. Lists
  show no counts, so this needs no change.

## French (fr)

Checked on 2026-10-24 by Claude against the checklist above, reading as a French speaker would, plus a
test of the matcher. The test scored every fr form and about 400 answers that French speakers type, each
against its own concept and against all the others. This is not the native review the spec asks for: a
French speaker still confirms it and signs the status table.

**Words.** All 140 concepts are ordinary supermarket items in France, Belgium and Switzerland. Quebec has
them too, except whisky and champagne, which are sold at the SAQ. Every form is spelt correctly,
including the 1990 spellings "ognon" and "shampoing", and nothing is offensive or odd. The display forms
are what a French shopping list says. Single fruit and vegetables stay in the singular, as "pomme" does
in the spec's example row; the plural is always accepted.

The questions above:

- "lardons" for bacon: right, it is what French cooks buy. "bacon" and "lard" stay accepted; "lardon" and
  "poitrine fumée" are added.
- "donut" for doughnut: right, it is the usual word today. "beignet" stays, and Quebec's "beigne" is
  added.
- "piment" for chilli: right in France. In Quebec "piment" often means a bell pepper, so "piment fort" is
  added.
- "chips" for crisps: right everywhere. Quebec's "croustilles" is added.

**Near-synonyms.** The pairs a player could mix up are in groups: citron and citron vert, pastèque and
melon, raisin and raisins secs, the three breads, huile and huile d’olive, eau and eau gazeuse, thé and
thé vert, and œufs and bœuf. "gâteau" (cake) and "biscuits" stay apart: the shown words are clearly
different, and "gâteaux secs" belongs only to the biscuits.

**Forms added.** Some plurals counted only as a typo ("salades", "fromages"), and "pains" did not count
at all, because "pain" is too short for a typo. Some common names were missing.

| id | key | fr after the review | Added |
|---|---|---|---|
| 6 | lime | citron vert;citrons verts;lime;limes | "lime", from Quebec and more and more in France |
| 12 | watermelon | pastèque;pastèques;melon d’eau | Quebec |
| 27 | lettuce | salade;salades;laitue;laitues | plurals |
| 32 | corn | maïs;maïs doux;épi de maïs;épis de maïs;blé d’Inde | the tin says "maïs doux"; Quebec "blé d’Inde" |
| 44 | chilli | piment;piments;piment fort;piments forts | see above |
| 46 | soy_milk | lait de soja;boisson au soja;boisson soja;lait de soya | the carton says "boisson soja"; Quebec "soya" |
| 48 | cheese | fromage;fromages | plural |
| 49 | yoghurt | yaourt;yaourts;yogourt;yogourts;yoghourt;yoghourts | plurals |
| 51 | cream | crème;crème fraîche;crème liquide;crème fleurette | the cream a French kitchen buys most |
| 53 | bread | pain;pains | plural |
| 55 | bread_rolls | petits pains;petit pain;pistolets;pistolet | Belgium |
| 57 | doughnut | donut;donuts;beignet;beignets;beigne;beignes;doughnut;doughnuts | Quebec, and the English spelling |
| 59 | biscuits | biscuits;biscuit;gâteaux secs;gâteau sec;cookies;cookie | singular of "gâteaux secs" |
| 65 | mince | viande hachée;haché;hachée;bœuf haché | the usual name on the pack |
| 71 | bacon | lardons;lardon;bacon;lard;poitrine fumée | singular, and the cut |
| 74 | squid | calamar;calamars;calmar;calmars;encornet;encornets | plurals |
| 97 | water | eau;eau minérale;eau plate | "plate", as opposed to "gazeuse" |
| 99 | juice | jus;jus de fruits;jus de fruit;jus d’orange;jus de pomme | singular |
| 101 | wine | vin;vin rouge;vin blanc;vin rosé | rosé |
| 103 | cola | cola;coca | the everyday word |
| 108 | crisps | chips;croustilles | Quebec |
| 113 | walnuts | noix;cerneaux de noix | shelled walnuts, as sold |
| 117 | marshmallows | guimauves;guimauve;marshmallows;marshmallow;chamallows;chamallow | the everyday word in France |
| 121 | bin_bags | sacs poubelle;sac poubelle;sacs poubelles;sacs à ordures | the other plural; Quebec |
| 128 | clothes_pegs | pinces à linge;pince à linge;épingles à linge;épingle à linge | Quebec |
| 129 | toothpaste | dentifrice;pâte dentifrice | Quebec |
| 130 | toothbrush | brosse à dents;brosses à dents;brosse à dent | plural, and the common spelling with "dent" |
| 134 | sun_cream | crème solaire;écran solaire | as common as "crème solaire" |

No display form changes. The rules of `tests/words.rs` hold with these forms: no two concepts share a
form, and no typo leads from one concept to another.

**Not added, on purpose.** "gomme" (Quebec for chewing gum) is one letter from "pomme". "rosé" on its own
normalizes to "rose", a flower. "baguette" is one kind of bread, and the spec keeps it out. "steak haché"
is a patty, not mince. "nouilles" are noodles, not pasta. "petits gâteaux" would bring the biscuits too
close to "gâteau". "coca" and "chamallow" come from brands (Coca-Cola, Haribo Chamallows), but they are
the everyday words, as "coton-tige" already is.

**Typos.** As the spec says, one letter off still counts, so an answer can land on an item the player did
not mean. Quebec's "gomme" counts for "pomme" when apples are on the list, and "pâté" counts for "pâtes".
Both cases are rare, and no typo can reach another concept of the list.

**Articles.** French speakers often write the article: "du lait", "des pommes", "l’ail". In French the
matcher drops one leading article (le, la, les, l’, un, une, des, du, de, de la, de l’, d’), so these
answers count as right. No French form begins with an article.

## Review: Japanese (ja)

AI review by Claude on 2026-10-24 against the checklist. A native speaker still has to sign it off.

- **Words and display forms.** All 140 concepts are everyday words, spelled the usual way, and every
  display form is what a Japanese shopping list says. No display form changes. Nothing is offensive or
  odd. The `tea` group matters in Japanese: お茶 alone usually means green tea (緑茶).
- **The points above.** 長ねぎ for leek is right: the European leek (リーキ) is rare, and 白ねぎ is the word in
  western Japan. コーン is how canned and frozen sweetcorn is labelled. ポテチ is colloquial but universal. 制汗剤
  is the general shop word for deodorant. フロス stays because 糸ようじ is a trademark of Kobayashi
  Pharmaceutical; it remains an accepted form. 飴 is right for sweets, because お菓子 would also cover
  crisps, chocolate and biscuits. お米 is right for rice. Pear shows 梨, because なし alone reads as 無し
  ("none").
- **Accepted forms added.** IME candidates of display forms, so the candidate a person picks never costs
  a point: じゃが芋, さつま芋, 茸, 長葱, とり肉, 洗濯挟み, and オリーブゆ, the reading of オリーブ油. Readings of listed kanji forms:
  とり, はくりきこ, カレーこ, あかワイン, しろワイン, いたチョコ, せんたくせんざい, はなたば, かみおむつ, いとようじ. Other common spellings: アボガド,
  ウィンナー, キウイフルーツ, 桜桃, 李, 洋梨, ようなし, なすび, 白ねぎ, しろねぎ, ツナ缶, つなかん, 醬油, 石鹼, 干し葡萄, 歯磨き, はみがき, 髭剃り, ひげ剃り, 糸楊枝.
- **Corrected.** サラダ油 is read サラダゆ, so the form サラダあぶら is replaced.
- **Checked.** These look-alikes stay apart in the matcher. Each is exact-only (three kana or fewer, or
  kanji) or more than one typo away: もも and すもも, なす and なし, ハム and ガム, ライス, ライム and アイス, からし and 唐辛子, ねぎ
  and 玉ねぎ, コーン and コーンフレーク. ー, small kana, the ideographic space, 。 and ・ behave as typed through a
  Japanese input method. A missing ー in a form of four kana or more is a forgiven typo (コーヒ, マンゴ).
  Half-width katakana (ﾘﾝｺﾞ, from F8 on a desktop) and kana typed apart from their sound mark (decomposed
  text) match as well; the code review added both to the matcher.
- **For the owner.** Fresh coconuts are rare in Japanese supermarkets and mostly found in import shops.
  Everyone knows ココナッツ, so it can stay.

## Review: Chinese (zh)

**Scope.** All 140 zh cells and the matcher, reviewed the way a mainland shopper typing with a pinyin
input method would see them. This review was written by Claude, an AI, not by a human native speaker. The
answers were checked with a scratch harness that compiles the crate's own `words.rs` and `matcher.rs` and
runs 401 answers people really type, from 苹果 and 蘋果 to `ping guo`, 一瓶牛奶, 车厘子 and 尿不湿.

**Result.**

- The display forms are the everyday mainland words, spelled right and at most three characters long.
  None is offensive or culturally odd. Two display forms change (below).
- Every display form has its traditional form and its toneless pinyin.
- Pinyin is accepted with spaces, apostrophes (`xi'gua`), tone marks (`píngguǒ`), ü as u or v (`lǜchá`,
  `lvcha`) and full-width letters. Chinese punctuation and the ideographic space are ignored.
- Characters must match exactly, as the spec says: 萍果 and 西红市 do not count.

**Display forms changed.**

- cream: 淡奶油 instead of 奶油. Some people also use 奶油 for butter (人造奶油, 奶油面包; in Taiwan 奶油 means butter),
  and supermarkets sell cream as 淡奶油. 奶油 is still accepted.
- clothes pegs: 晾衣夹 instead of 衣夹. For `yijia` the input method offers 衣架 (clothes hanger) first. 衣夹 is
  still accepted.

**Forms added.**

- One reading corrected: 芫荽 is read yánsui, so `yansui` is added. `yuansui`, which many people say,
  stays.
- The pinyin of every listed alternative, so typing the reading always counts. The missing ones were
  cheng, digua, rulao, dan, dannaiyou, xiannaiyou, xiaocanbao, mianbaoxie, ji, jiaorou, tunnayu, lachang,
  yifen, shatang, shiyan, rekeke, bingjiling, liangyijia, xifalu, zhihanji and xiangtilu.
- 18 missing traditional forms: 馬芬, 燕麥片, 麵包屑, 雞, 絞肉, 鮭魚, 吞拿魚, 蝦仁, 臘腸, 墨魚, 食鹽, 比薩, 晾衣夾, 洗髮露, 止汗劑, 除臭劑,
  香體露, 刮鬍刀.
- Common synonyms, each with its traditional form and pinyin:

  | Concept | Added |
  |---|---|
  | cherry | 车厘子 |
  | lime | 青柠檬 |
  | melon | 蜜瓜 |
  | pear | 雪梨 |
  | carrot | 红萝卜 |
  | garlic | 蒜头 |
  | bell pepper | 菜椒 (same pinyin as 彩椒, so the input method may offer it first), 灯笼椒 |
  | cabbage | 包菜, 包心菜 |
  | celery | 西芹 |
  | parsley | 法香, 洋香菜 |
  | cheese | 起司 |
  | condensed milk | 炼奶 |
  | bread rolls | 小面包 |
  | muffin | 麦芬 |
  | prawns | 大虾 |
  | sausages | 火腿肠 |
  | soy sauce | 生抽, 老抽 |
  | mayonnaise | 沙拉酱 |
  | tea | 红茶 |
  | water | 纯净水 |
  | juice | 橙汁, 苹果汁 |
  | tissues | 抽纸, 面巾纸 |
  | clothes pegs | 衣服夹 |
  | sun cream | 防晒, 防晒乳 |
  | cotton buds | 棉棒 |
  | nappies | 尿不湿 |

**Groups added or extended** (rows for the group table):

| Group | Concepts | Why |
|---|---|---|
| `melon` | watermelon, melon, sweet potato | similar fruit; pinyin "xigua" (西瓜) and "digua" (地瓜) are one letter apart |
| `orange` | orange, mandarin, juice | similar fruit; pinyin "chengzi" (橙子) and "chengzhi" (橙汁) are one letter apart |
| `sweets` (was `mangguo_tangguo`) | mango, sweets, sugar, and candles from the English review | pinyin "mangguo" and "tangguo" are one letter apart; Chinese 糖 means both sugar and sweets |
| `bocai_baocai` | spinach, cabbage | pinyin "bocai" (菠菜) and "baocai" (包菜) are one letter apart |
| `herbs` | coriander, parsley | they look alike, and Chinese also calls parsley 洋香菜 |

With these changes the zh column passes every rule of `tests/words.rs`: unique forms, no forgiven typo
between concepts of different groups, groups of at least two, and at least 3 concepts outside groups in
every category.

**Matcher: quantities.** People write quantities on a Chinese list: 一瓶牛奶, 两盒鸡蛋, 3个苹果, 鸡蛋两盒, 一斤半苹果. Since
characters must match exactly, none of these used to count. Step 7 of `normalize` (Chinese only) now
drops a quantity before or after the item: numerals or digits followed by a measure word, or trailing
digits as in "牛奶x2". A numeral needs a measure word after it, so 三文鱼 and 四季豆 stay whole.

**Known limits, accepted.**

- 梨子 is accepted for pear, but its pinyin "lizi" is plum (李子). Agreed.
- Raw pinyin of a synonym the list does not have can still be one letter from another item: "xianggu"
  (香菇) counts as melon ("xianggua"). Listing 香菇 would put mushrooms into the melon group. A player who
  saw 蘑菇 types 蘑菇.
- 芥末 alone usually means wasabi in China. It is accepted for mustard because the list has no wasabi.
- 墨鱼 (cuttlefish) is accepted for squid, 腊肠 and 火腿肠 for sausages, and 杏仁 for almonds, because people use
  them that way.
- Pinyin with tone numbers (`ping2guo3`) is not accepted. Native speakers do not type it.

**The points for the reviewers.**

- 哈密瓜 for melon: yes; 蜜瓜 is added.
- 意面 for pasta: yes.
- 止汗露 for deodorant: yes.
- 餐包 for bread rolls: yes; 小面包 is added.
- 玛芬 for muffin: yes, as Starbucks China writes it; 麦芬 is added.
- 黄芥末 for mustard: yes.
- "lizi" left out: agreed.

**Sold everywhere?** Every concept is sold in ordinary supermarkets in mainland cities except parsley
(欧芹). Most shoppers do not buy it, and many mix it up with 香菜 or 芹菜, which is why it gets the aliases and
the `herbs` group. 青花鱼, 玛芬, 止汗露 and 香槟 are found mostly in larger supermarkets, but people know them. For
the owner: pork (猪肉), the meat people in China buy most, is missing because of the German length limit.
It is worth a look before the freeze.

## Groups

Groups keep near-duplicates out of the same list, and pairs whose forms are one typo apart:

| Group | Concepts | Why |
|---|---|---|
| `orange` | orange, mandarin, juice | similar fruit; pinyin "chengzi" (橙子) and "chengzhi" (橙汁) are one letter apart |
| `lemon` | lemon, lime | similar fruit |
| `melon` | watermelon, melon, sweet potato | similar fruit; pinyin "xigua" (西瓜) and "digua" (地瓜) are one letter apart |
| `grapes` | grapes, raisins | French "raisin" means grape |
| `bread` | bread, sliced bread, bread rolls | similar bread |
| `oil` | oil, olive oil | similar oil |
| `water` | water, fizzy water | similar drink |
| `tea` | tea, green tea | similar drink |
| `wine` | wine, champagne | champagne is accepted as "sparkling wine" |
| `oeuf_boeuf` | eggs, beef | French "œuf" and "bœuf" are one letter apart |
| `donatsu_pinatsu` | doughnut, peanuts | Japanese ドーナツ and ピーナツ are one letter apart |
| `sweets` | mango, sweets, sugar, candles | pinyin "mangguo"/"tangguo" and English "candies"/"candles" are one letter apart; Chinese 糖 means both sugar and sweets |
| `bocai_baocai` | spinach, cabbage | pinyin "bocai" (菠菜) and "baocai" (包菜) are one letter apart |
| `herbs` | coriander, parsley | they look alike, and Chinese also calls parsley 洋香菜 |
| `flour` | flour, flowers | English homophones |

An answer that is exactly a form of another concept counts as that concept, never as a typo: "ドーナツ"
is not a misspelt "ピーナツ".

## Freeze

`groceries_v1_is_frozen` pins the SHA-256 of the file. Until the first release that ships the contest,
a reviewed correction changes the file, the pinned hash and `tests/golden/list.json` (the lists of a fixed
seed) in the same commit. From that release on, version 1 never changes: corrections go into
`groceries-v2.tsv` with `CONTENT = 2`, next to version 1.
