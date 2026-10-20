#!/usr/bin/env python3
"""Translates the built-in palaces "Locora lernen" and "Formeln verstehen".

Usage: SPLINELY_CLI=splinely-cli python3 assets/tutorial/src/i18n.py

The German sources (assets/tutorial/locora-lernen.locora.toml and
assets/tutorial/formel-guide/guide.locora.toml, with their .sply pictures) stay
the base. For each language this writes assets/tutorial/i18n/<lang>/:
- the two palace sources, with every German text replaced;
- the pictures whose visible text is translated (node text, via splinely-cli).
It also writes crates/locora-runtime/src/palace_i18n.rs, which embeds those files.
The script stops with an error if a German text has no translation.
"""
import json, os, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
ASSETS = os.path.join(ROOT, "assets", "tutorial")
OUT = os.path.join(ASSETS, "i18n")
RUST = os.path.join(ROOT, "crates", "locora-runtime", "src", "palace_i18n.rs")
CLI = os.environ.get("SPLINELY_CLI", "splinely-cli")
LANGS = ["en", "fr", "ja", "zh"]

# German text -> translation per language, in the order en, fr, ja, zh.
TEXTS = {
    # Tutorial: name, rooms, places, items, quiz options, note.
    "Locora lernen": ["Learn Locora", "Apprendre Locora", "Locora を学ぶ", "学习 Locora"],
    "Eingangshalle": ["Entrance hall", "Hall d’entrée", "玄関ホール", "入口大厅"],
    "Bibliothek": ["Library", "Bibliothèque", "図書室", "图书馆"],
    "Garten": ["Garden", "Jardin", "庭園", "花园"],
    "Das Tor – was ist ein Gedächtnispalast?": [
        "The gate – what is a memory palace?", "La porte – qu’est-ce qu’un palais de mémoire ?",
        "門 – 記憶の宮殿とは何か？", "大门——什么是记忆宫殿？"],
    "Der Teppich – was gibt die Reihenfolge vor?": [
        "The rug – what sets the order?", "Le tapis – qu’est-ce qui fixe l’ordre ?",
        "じゅうたん – 順番を決めるのは？", "地毯——什么决定顺序？"],
    "Die Laterne – was legst du an jeden Ort?": [
        "The lantern – what do you put at each place?", "La lanterne – que places-tu à chaque endroit ?",
        "ランタン – 各場所に何を置く？", "灯笼——你在每个位置放什么？"],
    "Das Regal – welche Bilder bleiben hängen?": [
        "The shelf – which pictures stick?", "L’étagère – quelles images restent ?",
        "本棚 – どんなイメージが残る？", "书架——哪些画面会留下？"],
    "Der Leseplatz – wie übst du?": [
        "The reading nook – how do you practise?", "La place de lecture – comment t’entraînes-tu ?",
        "読書コーナー – どう練習する？", "阅读角——你怎么练习？"],
    "Die Uhr – wann wiederholst du?": [
        "The clock – when do you review?", "L’horloge – quand révises-tu ?",
        "時計 – いつ復習する？", "时钟——什么时候复习？"],
    "Der Brunnen – wie bewertest du eine Antwort?": [
        "The fountain – how do you rate an answer?", "La fontaine – comment évalues-tu une réponse ?",
        "噴水 – 答えをどう評価する？", "喷泉——你怎么给答案打分？"],
    "Der Baum – mit welchem Befehl legst du einen eigenen Palast an?": [
        "The tree – which command creates your own palace?",
        "L’arbre – avec quelle commande crées-tu ton propre palais ?",
        "木 – どのコマンドで自分の宮殿を作る？", "大树——用哪个命令创建自己的宮殿？"],
    "Das Glühwürmchen – womit übst du, was fällig ist?": [
        "The firefly – what do you use to practise what is due?",
        "La luciole – avec quoi t’entraînes-tu sur ce qui est dû ?",
        "ホタル – 期限のあるものの練習には何を使う？", "萤火虫——用什么练习到期的内容？"],
    "Ein Ort, den du gut kennst": ["A place you know well", "Un lieu que tu connais bien", "よく知っている場所", "一个你很熟悉的地方"],
    "Die Route durch die Orte": ["The route through the places", "Le parcours à travers les lieux", "場所をたどるルート", "穿过各个位置的路线"],
    "Ein lebhaftes Bild": ["A vivid picture", "Une image vivante", "鮮やかなイメージ", "一个生动的画面"],
    "Je seltsamer, desto besser": ["The stranger, the better", "Plus c’est bizarre, mieux c’est", "奇妙なほど良い", "越奇怪越好"],
    "Route abgehen und jeden Ort abrufen": ["Walk the route and recall every place", "Parcourir le trajet et rappeler chaque lieu", "ルートをたどって、すべての場所を思い出す", "沿路线走一遍，回想每一个位置"],
    "Kurz bevor du vergisst": ["Just before you forget", "Juste avant d’oublier", "忘れる直前", "就在快要忘记之前"],
    "Ehrlich, von 1 bis 4": ["Honestly, from 1 to 4", "Honnêtement, de 1 à 4", "正直に、1〜4で", "诚实地打 1 到 4 分"],
    "Immer 4, dann geht es schneller": ["Always 4, then it goes faster", "Toujours 4, ça va plus vite", "いつも4にすれば速い", "总是打 4，这样更快"],
    "Gar nicht": ["Not at all", "Pas du tout", "まったくしない", "完全不用"],
    "Aus dem Baum wächst ein neuer Palast – „new“ steht auf dem Schild am Stamm": [
        "A new palace grows from the tree – “new” is on the sign on the trunk",
        "Un nouveau palais pousse de l’arbre – « new » est écrit sur le panneau du tronc",
        "木からは新しい宮殿が育つ – 幹の札には「new」と書かれている",
        "大树会长出一座新宮殿——树干的牌子上写着“new”"],
    # Formula guide: name, rooms, places, items, notes, values.
    "Formeln verstehen": ["Understand the formulas", "Comprendre les formules", "数式を理解する", "理解公式"],
    "Schreibstube": ["Writing room", "Bureau d’écriture", "書斎", "书房"],
    "Werkstatt": ["Workshop", "Atelier", "工房", "工坊"],
    "Das Eingabebuch – wie liest eine Formel eine Eingabe?": [
        "The input book – how does a formula read an input?",
        "Le livre des entrées – comment une formule lit-elle une entrée ?",
        "入力ブック – 数式は入力をどう読む？", "输入册——公式如何读取输入？"],
    "Der Rechentisch – was ergibt Integer($zahl) + 1 bei zahl = 3?": [
        "The calculating table – what does Integer($zahl) + 1 give when zahl = 3?",
        "La table de calcul – que donne Integer($zahl) + 1 quand zahl = 3 ?",
        "計算台 – zahl = 3 のとき Integer($zahl) + 1 はいくつ？",
        "计算台——当 zahl = 3 时，Integer($zahl) + 1 等于多少？"],
    "Das Register – welches Merkwort liefert Major[4]?": [
        "The register – which mnemonic word does Major[4] give?",
        "Le registre – quel mot-mémoire donne Major[4] ?",
        "索引 – Major[4] はどの覚え語を返す？", "索引——Major[4] 给出哪个记忆词？"],
    "Das Etikett – welches Feld ersetzt eine Bindung?": [
        "The label – which field replaces a binding?", "L’étiquette – quel champ remplace une liaison ?",
        "ラベル – どの欄がバインディングの代わりになる？", "标签——哪个字段可以替代绑定？"],
    "Die Weiche – wann ist ein Ort aktiv?": [
        "The switch – when is a place active?", "L’aiguillage – quand un lieu est-il actif ?",
        "分岐器 – 場所はいつ有効になる？", "道岔——什么时候一个位置是激活的？"],
    "Das Tor – wo schreibst du die Formel für den Ort?": [
        "The gate – where do you write the formula for the place?",
        "La porte – où écris-tu la formule du lieu ?", "門 – 場所の数式はどこに書く？",
        "大门——在哪里写位置的公式？"],
    "Wenn die active-Formel true ergibt": ["When the active formula gives true", "Quand la formule active donne true", "active の数式が true になるとき", "active 公式结果为 true 时"],
    "In bindings der locora.toml-Datei": ["In the bindings of the locora.toml file", "Dans bindings du fichier locora.toml", "locora.toml ファイルの bindings の中", "写在 locora.toml 文件的 bindings 里"],
    "Ein Dollarzeichen vor dem Eingabenamen liest dessen aktuellen Wert.": [
        "A dollar sign before an input name reads its current value.",
        "Un signe dollar devant le nom d’une entrée lit sa valeur actuelle.",
        "入力名の前のドル記号は、その現在の値を読みます。",
        "输入名前面的美元符号会读取它当前的值。"],
    "Dieser Ort berechnet seine Antwort selbst: Integer($zahl) + 1. Der Startwert von zahl ist 3.": [
        "This place works out its answer itself: Integer($zahl) + 1. The starting value of zahl is 3.",
        "Ce lieu calcule lui-même sa réponse : Integer($zahl) + 1. La valeur de départ de zahl est 3.",
        "この場所は答えを自分で計算します: Integer($zahl) + 1。zahl の初期値は 3 です。",
        "这个位置自己算出答案：Integer($zahl) + 1。zahl 的初始值是 3。"],
    "Dieser Ort liest seine Antwort aus Major[\"4\"]. Der Schlüssel steht als Text in eckigen Klammern.": [
        "This place reads its answer from Major[\"4\"]. The key is written as text in square brackets.",
        "Ce lieu lit sa réponse dans Major[\"4\"]. La clé est écrite en texte entre crochets.",
        "この場所は Major[\"4\"] から答えを読みます。キーは角かっこの中に文字として書きます。",
        "这个位置从 Major[\"4\"] 读取答案。键以文本形式写在方括号里。"],
    "Eine item-Bindung berechnet den Inhalt eines Ortes. Auch place, x, y, visible und active lassen sich binden.": [
        "An item binding computes the content of a place. place, x, y, visible and active can be bound too.",
        "Une liaison item calcule le contenu d’un lieu. place, x, y, visible et active peuvent aussi être liés.",
        "item のバインディングは場所の内容を計算します。place、x、y、visible、active もバインドできます。",
        "item 绑定计算一个位置的内容。place、x、y、visible 和 active 也可以绑定。"],
    "Zum Beispiel: Integer($zahl) > 2. Ein inaktiver Ort wird in der Journey übersprungen.": [
        "For example: Integer($zahl) > 2. An inactive place is skipped in the journey.",
        "Par exemple : Integer($zahl) > 2. Un lieu inactif est sauté dans le parcours.",
        "例: Integer($zahl) > 2。無効な場所は旅の中で飛ばされます。",
        "例如：Integer($zahl) > 2。未激活的位置会在旅程中被跳过。"],
    "Beispiel: bindings": ["Example: bindings", "Exemple : bindings", "例: bindings", "例：bindings"],
    "Reh": ["Rat", "Renard", "ライオン", "狼"],
    # Pictures: visible text (node text).
    "Nochmal": ["Again", "Encore", "もう一度", "再来"],
    "Schwer": ["Hard", "Difficile", "難しい", "困难"],
    "Gut": ["Good", "Bien", "良い", "良好"],
    "Leicht": ["Easy", "Facile", "簡単", "简单"],
    "Bedingung": ["Condition", "Condition", "条件", "条件"],
    "true → üben": ["true → practise", "vrai → s’entraîner", "true → 練習", "true → 练习"],
    "Ort im Palast": ["Place in the palace", "Lieu dans le palais", "宮殿の中の場所", "宫殿中的位置"],
    "Formel → Inhalt": ["Formula → content", "Formule → contenu", "数式 → 内容", "公式 → 内容"],
    "Eingaben": ["Inputs", "Entrées", "入力", "输入"],
    "Name → Wert": ["Name → value", "Nom → valeur", "名前 → 値", "名称 → 值"],
    "Formeln im Palast": ["Formulas in the palace", "Formules dans le palais", "宮殿の中の数式", "宫殿中的公式"],
    "Umwandeln": ["Convert", "Convertir", "変換", "转换"],
    "Zahl + 1": ["Number + 1", "Nombre + 1", "数 + 1", "数 + 1"],
    "Register Major": ["Major register", "Registre Major", "Major の索引", "Major 索引"],
    "Schlüssel 4 → Reh": ["Key 4 → Rat", "Clé 4 → Renard", "キー 4 → ライオン", "键 4 → 狼"],
}

# Source files: German base, and the pictures with visible text (by name).
TUTORIAL_TOML = ("locora-lernen.locora.toml", os.path.join(ASSETS, "locora-lernen.locora.toml"))
GUIDE_TOML = ("guide.locora.toml", os.path.join(ASSETS, "formel-guide", "guide.locora.toml"))
PICTURES = {
    "brunnen.sply": os.path.join(ASSETS, "brunnen.sply"),
    "aktiv.sply": os.path.join(ASSETS, "formel-guide", "aktiv.sply"),
    "bindung.sply": os.path.join(ASSETS, "formel-guide", "bindung.sply"),
    "eingabe.sply": os.path.join(ASSETS, "formel-guide", "eingabe.sply"),
    "quelle.sply": os.path.join(ASSETS, "formel-guide", "quelle.sply"),
    "rechnen.sply": os.path.join(ASSETS, "formel-guide", "rechnen.sply"),
    "tabelle.sply": os.path.join(ASSETS, "formel-guide", "tabelle.sply"),
}

LANG_INDEX = {lang: index for index, lang in enumerate(LANGS)}


def translate_toml(text, lang, used):
    """Replace every German text of a palace source; `used` collects the ones found."""
    index = LANG_INDEX[lang]
    # Longest first, so a text that contains another one is replaced whole.
    for german in sorted(TEXTS, key=len, reverse=True):
        if german in text:
            used.add(german)
            text = text.replace(german, TEXTS[german][index])
    return text


def picture_texts(path):
    """The visible text nodes of a picture: node id by text."""
    out = subprocess.run([CLI, "cat", path], capture_output=True, text=True, check=True)
    nodes = json.loads(out.stdout).get("nodes", [])
    found = {}
    for node in nodes:
        kind = node.get("kind")
        if isinstance(kind, dict) and isinstance(kind.get("text"), str):
            found.setdefault(kind["text"], []).append(node["id"])
    return found


def translate_picture(name, source, lang, used):
    """Copy a picture and set its translated text nodes; returns the new file path."""
    target = os.path.join(OUT, lang, name)
    shutil.copyfile(source, target)
    index = LANG_INDEX[lang]
    for text, ids in picture_texts(source).items():
        if text in TEXTS and text.strip() and not text.strip().isdigit():
            used.add(text)
            for node_id in ids:
                subprocess.run([CLI, "node", "update", target, node_id, "--text", TEXTS[text][index]],
                               capture_output=True, text=True, check=True)
    return target


def main():
    for lang in LANGS:
        os.makedirs(os.path.join(OUT, lang), exist_ok=True)
        used = set()
        for name, path in (TUTORIAL_TOML, GUIDE_TOML):
            with open(path, encoding="utf-8") as f:
                text = translate_toml(f.read(), lang, used)
            with open(os.path.join(OUT, lang, name), "w", encoding="utf-8") as f:
                f.write(text)
        for name, source in PICTURES.items():
            translate_picture(name, source, lang, used)
        missing = set(TEXTS) - used
        if missing:
            sys.exit(f"{lang}: German texts without a place in the sources: {sorted(missing)}")

    entries = []
    for lang in LANGS:
        for name in sorted(os.listdir(os.path.join(OUT, lang))):
            rel = os.path.join("assets", "tutorial", "i18n", lang, name)
            entries.append(f'    ("{lang}", "{name}", include_bytes!("../../../{rel}")),')
    with open(RUST, "w", encoding="utf-8") as f:
        f.write("//! Translated sources and pictures of the built-in palaces, written by\n")
        f.write("//! assets/tutorial/src/i18n.py. Do not edit by hand.\n\n")
        f.write("/// (language, file name, bytes)\n")
        f.write("pub const FILES: &[(&str, &str, &[u8])] = &[\n")
        f.write("\n".join(entries) + "\n];\n\n")
        f.write("/// The translated bytes of a file, if the language has one.\n")
        f.write("pub fn file(lang: &str, name: &str) -> Option<&'static [u8]> {\n")
        f.write("    FILES\n        .iter()\n")
        f.write("        .find(|(language, file, _)| *language == lang && *file == name)\n")
        f.write("        .map(|(_, _, bytes)| *bytes)\n}\n")
    print("translated", len(LANGS), "languages into", os.path.relpath(OUT, ROOT))


if __name__ == "__main__":
    main()
