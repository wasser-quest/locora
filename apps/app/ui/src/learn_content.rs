//! The lessons of „Mnemotechnik lernen“ (see learn.rs): what each character explains, an
//! example, a small exercise with a check, and how to do it in Locora. German texts are the
//! keys of the catalog; every lesson id matches `locora_app_api::LEARN_LESSONS`.

use crate::i18n::t;

/// Who explains a lesson: Simo or one of his friends (assets/mascot).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Simo,
    Mina,
    Cami,
    Bruno,
    Hafiza,
}

/// The tile groups on the page, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Start,
    Pictures,
    Route,
    Extra,
}

/// What „So machst du es in Locora“ offers besides its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    /// Open the tutorial palace „Locora lernen“.
    Tutorial,
    /// Start the first own palace from a photo.
    FirstPalace,
    /// Open the settings (reminders).
    Settings,
}

pub struct Lesson {
    pub id: &'static str,
    pub who: Who,
    pub group: Group,
    pub title: &'static str,
    pub teaser: &'static str,
    pub paragraphs: [&'static str; 3],
    pub example: &'static str,
    pub exercise: &'static str,
    pub question: &'static str,
    /// The right answer comes first here; the page mixes the order.
    pub options: [&'static str; 3],
    /// Shown after a wrong answer.
    pub hint: &'static str,
    pub in_locora: Option<&'static str>,
    pub action: Action,
}

/// All lessons in order, in the current language.
pub fn lessons() -> Vec<Lesson> {
    vec![
        Lesson {
            id: "basics",
            who: Who::Simo,
            group: Group::Start,
            title: t("Was ist die Loci-Methode?"),
            teaser: t("Ein Gedächtnispalast in drei Sätzen."),
            paragraphs: [
                t(
                    "Bei der Loci-Methode legst du in Gedanken alles, was du dir merken willst, an Orte, die du gut kennst – zum Beispiel in deiner Wohnung.",
                ),
                t(
                    "Jeder Ort bekommt ein Bild. Später gehst du die Orte im Kopf der Reihe nach ab und findest dort deine Bilder wieder.",
                ),
                t(
                    "Das klappt so gut, weil unser Gedächtnis Orte und Bilder besonders leicht behält. Den Weg durch deine Wohnung kennst du ja schon auswendig.",
                ),
            ],
            example: t(
                "Beispiel: Auf deiner Einkaufsliste stehen Milch, Brot und Äpfel. Auf deiner Fußmatte steht eine Kuh, auf dem Sofa liegt ein riesiges Brot, und in der Badewanne schwimmen Äpfel.",
            ),
            exercise: t(
                "Schließ kurz die Augen und geh in Gedanken von deiner Haustür in die Küche. Welche drei Dinge siehst du unterwegs?",
            ),
            question: t("Was ist der Kern der Loci-Methode?"),
            options: [
                t("Bilder an bekannte Orte legen und die Orte der Reihe nach abgehen"),
                t("Einen Text so oft lesen, bis man ihn kann"),
                t("Alles auf Zettel schreiben"),
            ],
            hint: t(
                "Fast! Denk an Simos Beispiel: die Kuh auf der Fußmatte, das Brot auf dem Sofa …",
            ),
            in_locora: Some(t(
                "In Locora ist ein Palast ein Foto oder Video von einem Ort, den du kennst. Darauf setzt du Punkte – das sind deine Orte. Das Tutorial zeigt es dir an einem fertigen Palast.",
            )),
            action: Action::Tutorial,
        },
        Lesson {
            id: "visualize",
            who: Who::Cami,
            group: Group::Pictures,
            title: t("Visualisieren: lebendige Bilder"),
            teaser: t("Mach aus Wörtern Bilder, die du fast sehen kannst."),
            paragraphs: [
                t(
                    "Unser Gedächtnis merkt sich Bilder viel leichter als Wörter. Mach darum aus allem, was du behalten willst, ein Bild.",
                ),
                t(
                    "Das Bild soll konkret sein: nicht „Obst“, sondern ein bestimmter, roter, glänzender Apfel. Je genauer du es siehst, desto besser.",
                ),
                t(
                    "Nimm dir für jedes Bild zwei, drei Sekunden: Welche Farbe hat es? Wie groß ist es? Wo genau liegt es?",
                ),
            ],
            example: t(
                "Statt „Termin beim Zahnarzt“: Ein riesiger weißer Zahn klingelt an deiner Tür, einen Kalender unter dem Arm.",
            ),
            exercise: t(
                "Stell dir eine Zitrone vor. Sieh ihre gelbe, raue Schale, schneide sie in Gedanken auf und rieche daran.",
            ),
            question: t("Welches Bild merkt man sich leichter?"),
            options: [
                t("Ein knallroter, glänzender Apfel auf deinem Kopfkissen"),
                t("Das Wort „Obst“"),
                t("Irgendein Apfel irgendwo"),
            ],
            hint: t("Denk an Cami: konkret, farbig und an einem bestimmten Ort."),
            in_locora: Some(t(
                "Im Editor schreibst du dein Bild in das Feld „Merkhilfe“. Du kannst an einen Ort auch ein eigenes Bild legen.",
            )),
            action: Action::None,
        },
        Lesson {
            id: "exaggerate",
            who: Who::Cami,
            group: Group::Pictures,
            title: t("Übertreiben & Absurdes"),
            teaser: t("Riesig, ganz viele, verrückt – Ungewöhnliches bleibt hängen."),
            paragraphs: [
                t(
                    "Was ungewöhnlich ist, fällt uns auf – und was auffällt, merken wir uns. Alltägliche Bilder vergessen wir schnell.",
                ),
                t(
                    "Darum: Übertreib! Mach Dinge riesig oder winzig, nimm statt einem gleich hundert, oder lass sie etwas tun, was sie nie tun würden.",
                ),
                t(
                    "Verrückt und lustig ist erlaubt und erwünscht. Deine Bilder sieht niemand außer dir.",
                ),
            ],
            example: t(
                "Du brauchst Briefmarken: Eine Briefmarke, so groß wie deine Haustür, klebt davor, und du musst dich daran vorbeiquetschen.",
            ),
            exercise: t(
                "Nimm einen Schlüssel und mach ihn in Gedanken so groß wie ein Auto. Wo parkt er?",
            ),
            question: t("Welches Bild ist am besten übertrieben?"),
            options: [
                t("Tausend kleine Tassen regnen auf dein Bett"),
                t("Eine Tasse steht im Schrank"),
                t("Eine Tasse auf dem Tisch"),
            ],
            hint: t("Ein Bild aus dem Alltag ist zu brav. Welches würde dich überraschen?"),
            in_locora: None,
            action: Action::None,
        },
        Lesson {
            id: "motion",
            who: Who::Cami,
            group: Group::Pictures,
            title: t("Bewegung & Handlung"),
            teaser: t("Ein Bild, das etwas tut, merkt man sich besser."),
            paragraphs: [
                t(
                    "Ein stilles Bild ist wie ein Foto. Ein Bild, das sich bewegt, ist wie ein kleiner Film – und Filme merken wir uns leichter.",
                ),
                t(
                    "Lass dein Bild etwas tun: Es springt, explodiert, tanzt, schmilzt oder rennt davon.",
                ),
                t(
                    "Am besten tut es etwas mit dem Ort: Die Milch spritzt über die Fußmatte, das Brot hüpft auf dem Sofa auf und ab.",
                ),
            ],
            example: t(
                "Statt „ein Buch auf dem Tisch“: Das Buch klappt auf und zu wie ein Maul und schnappt nach deiner Hand.",
            ),
            exercise: t(
                "Stell dir einen Ball auf deinem Küchentisch vor. Lass ihn hüpfen, immer höher, bis er an die Decke knallt.",
            ),
            question: t("Welches Bild nutzt Bewegung?"),
            options: [
                t("Ein Fisch zappelt im Waschbecken und spritzt Wasser an den Spiegel"),
                t("Ein Fisch liegt auf einem Teller"),
                t("Ein Bild von einem Fisch hängt an der Wand"),
            ],
            hint: t("Such das Bild, in dem etwas passiert."),
            in_locora: Some(t(
                "In Locora kannst du einen Palast auch aus einem Video machen. Dann bewegt sich sogar der Ort selbst.",
            )),
            action: Action::None,
        },
        Lesson {
            id: "senses",
            who: Who::Cami,
            group: Group::Pictures,
            title: t("Alle Sinne & Gefühle"),
            teaser: t("Hören, riechen, fühlen, lachen – je mehr, desto besser."),
            paragraphs: [
                t(
                    "Ein Bild wird stärker, wenn du nicht nur hinschaust. Was hörst du? Was riechst du? Wie fühlt es sich an?",
                ),
                t(
                    "Auch Gefühle helfen: Etwas Lustiges, Ekliges oder Rührendes merken wir uns besser als etwas, das uns egal ist.",
                ),
                t(
                    "Humor ist besonders stark. Wenn du über dein Bild schmunzeln musst, bleibt es meistens hängen.",
                ),
            ],
            example: t(
                "Kaffee besorgen: Auf dem Sofa blubbert ein See aus Kaffee, es duftet stark, die Polster sind warm und nass – und eine Ente schwimmt darauf herum.",
            ),
            exercise: t(
                "Denk an frisches Brot: Wie riecht es? Wie knackt die Kruste, wenn du hineinbeißt?",
            ),
            question: t("Was macht ein Bild stärker?"),
            options: [
                t("Geräusch, Geruch, Gefühl und etwas zum Lachen dazunehmen"),
                t("Es möglichst schnell ansehen"),
                t("Nur an das Wort denken"),
            ],
            hint: t("Cami nimmt mehr als nur die Augen dazu."),
            in_locora: Some(t(
                "In Locora kannst du an einen Ort einen Ton legen – zum Beispiel ein Geräusch, das zu deinem Bild passt.",
            )),
            action: Action::None,
        },
        Lesson {
            id: "link",
            who: Who::Cami,
            group: Group::Pictures,
            title: t("Verknüpfen: Bild und Ort"),
            teaser: t("Das Bild muss mit dem Ort etwas zu tun haben."),
            paragraphs: [
                t(
                    "Ein Bild, das nur neben dem Ort steht, fällt leicht wieder heraus. Verbinde es fest mit dem Ort.",
                ),
                t(
                    "Das Bild tut etwas mit dem Ort oder der Ort mit dem Bild: Der Toaster wirft die Brille hoch, die Tür klemmt den Schlüssel ein.",
                ),
                t("Frag dich: Was passiert, wenn genau dieses Ding genau hier ist?"),
            ],
            example: t(
                "Ort: Kühlschrank. Inhalt: Tomaten. Nicht „Tomaten liegen davor“, sondern: Du öffnest den Kühlschrank, und eine Lawine Tomaten rollt dir entgegen.",
            ),
            exercise: t(
                "Ort: dein Bett. Inhalt: ein Regenschirm. Denk dir aus, was der Regenschirm mit dem Bett macht.",
            ),
            question: t(
                "Ort: Waschmaschine. Inhalt: Katze. Welches Bild verknüpft beides am besten?",
            ),
            options: [
                t("Die Katze dreht sich in der Trommel und miaut im Schleudergang"),
                t("Eine Katze sitzt irgendwo in der Wohnung"),
                t("Neben der Waschmaschine hängt ein Katzenkalender"),
            ],
            hint: t("Ort und Inhalt sollen sich berühren und etwas zusammen tun."),
            in_locora: Some(t(
                "Im Editor hat jeder Ort die Felder „Ort“, „Inhalt“ und „Merkhilfe“. In die Merkhilfe schreibst du, wie beides zusammenhängt.",
            )),
            action: Action::None,
        },
        Lesson {
            id: "abstract",
            who: Who::Cami,
            group: Group::Pictures,
            title: t("Abstraktes greifbar machen"),
            teaser: t("Klangbilder, Ersatzwörter und Symbole."),
            paragraphs: [
                t(
                    "Wörter wie „Freiheit“ oder Namen wie „Herr Bergmann“ kann man nicht sehen. Dann brauchst du ein Ersatzbild.",
                ),
                t(
                    "Klangbild: Was klingt ähnlich? „Bergmann“ wird ein Mann mit Helm auf einem Berg. Ein fremdes Wort zerlegst du in Teile, die du sehen kannst.",
                ),
                t(
                    "Symbol: Was steht für die Idee? Freiheit wird ein Vogel, der aus dem Käfig fliegt; Liebe ein Herz; Gefahr ein Warnschild.",
                ),
            ],
            example: t(
                "Die Hauptstadt von Australien ist Canberra: Eine Kanne steht auf einem Berg, und ein Känguru springt heraus.",
            ),
            exercise: t(
                "Mach ein Bild für das Wort „Geduld“. Tipp: Wer ist besonders geduldig – eine Schnecke? Ein Angler?",
            ),
            question: t("Wie merkst du dir den Namen „Frau Fischer“ am besten?"),
            options: [
                t("Sie hält eine Angel, an der ein zappelnder Fisch hängt"),
                t("Ich sage den Namen leise vor mich hin"),
                t("Ich denke an das Wort „Name“"),
            ],
            hint: t("Mach aus dem Klang ein Bild, das man sehen kann."),
            in_locora: None,
            action: Action::None,
        },
        Lesson {
            id: "numbers",
            who: Who::Hafiza,
            group: Group::Pictures,
            title: t("Zahlen merken"),
            teaser: t("Aus Ziffern werden Bilder."),
            paragraphs: [
                t(
                    "Zahlen kann man schlecht sehen. Darum bekommt jede Ziffer ein festes Bild, das ihr ähnlich sieht.",
                ),
                t(
                    "Zahl-Form: 0 = Ei, 1 = Kerze, 2 = Schwan, 4 = Segelboot, 8 = Schneemann. Aus den Bildern baust du eine kleine Geschichte.",
                ),
                t(
                    "Es gibt auch Zahl-Reime, bei denen sich Zahl und Bild reimen. Profis nutzen das Major-System: Jede Ziffer wird ein Laut, und aus den Lauten werden Wörter. Das braucht Übung – ein guter nächster Schritt.",
                ),
            ],
            example: t(
                "PIN 1-2-8-0: Eine Kerze (1) sengt einem Schwan (2) die Federn an. Er flüchtet zu einem Schneemann (8), der ein Ei (0) ausbrütet.",
            ),
            exercise: t(
                "Merk dir 1-2-8-0 mit der Geschichte oben. Schau weg und sag die Zahl auf.",
            ),
            question: t("Für welche Ziffer steht der Schneemann?"),
            options: ["8", "1", "0"],
            hint: t("Schau dir die Form an: zwei Kugeln übereinander."),
            in_locora: Some(t(
                "In Locora kannst du jede Zahlen-Geschichte an einen Ort legen – eine lange Zahl auf mehrere Orte verteilt.",
            )),
            action: Action::None,
        },
        Lesson {
            id: "palace",
            who: Who::Bruno,
            group: Group::Route,
            title: t("Einen Palast wählen"),
            teaser: t("Der beste Palast ist ein Ort, den du schon kennst."),
            paragraphs: [
                t(
                    "Ein Gedächtnispalast ist ein echter Ort, den du gut kennst: deine Wohnung, das Haus deiner Eltern, dein Schulweg, dein Arbeitsplatz, ein Park.",
                ),
                t(
                    "Du musst ihn nicht neu lernen – du kennst ihn schon. Das ist der ganze Trick: Das Gedächtnis für Orte ist schon fertig.",
                ),
                t(
                    "Fang mit einem kleinen Palast an, zum Beispiel einem Zimmer. Später kannst du weitere Räume und Wege dazunehmen.",
                ),
            ],
            example: t(
                "Bruno nimmt seine Küche: Tür, Kühlschrank, Herd, Spüle, Fenster. Fünf Orte, die er im Schlaf findet.",
            ),
            exercise: t(
                "Denk an drei Orte, die du richtig gut kennst. Welcher davon wäre dein erster Palast?",
            ),
            question: t("Welcher Ort eignet sich am besten als erster Palast?"),
            options: [
                t("Deine eigene Wohnung"),
                t("Ein Hotel, in dem du einmal warst"),
                t("Ein Haus aus einem Film, den du kaum kennst"),
            ],
            hint: t("Nimm einen Ort, den du ohne Nachdenken durchgehen kannst."),
            in_locora: Some(t(
                "Mach ein Foto von einem Zimmer, das du gut kennst, und wähle es in Locora aus – daraus wird dein Palast.",
            )),
            action: Action::FirstPalace,
        },
        Lesson {
            id: "loci",
            who: Who::Bruno,
            group: Group::Route,
            title: t("Orte (Loci) auswählen"),
            teaser: t("Feste Reihenfolge, auffällig, verschieden, nicht zu nah."),
            paragraphs: [
                t(
                    "Die Orte in deinem Palast heißen Loci – lateinisch für „Orte“. Wähle sie in einer festen Reihenfolge, zum Beispiel immer im Uhrzeigersinn durch den Raum.",
                ),
                t(
                    "Gute Orte fallen auf und sind verschieden: Tür, Sofa, Lampe, Fenster. Zwei gleiche Stühle nebeneinander verwechselt man leicht.",
                ),
                t(
                    "Lass etwas Abstand zwischen den Orten. Für den Anfang reichen 5 bis 10 Orte pro Raum.",
                ),
            ],
            example: t(
                "Gut: Tür → Garderobe → Spiegel → Schuhregal → Lampe. Schlecht: Stuhl 1 → Stuhl 2 → Stuhl 3.",
            ),
            exercise: t(
                "Schau dich um: Wähle in deinem Zimmer fünf Orte im Uhrzeigersinn und zähl sie leise auf.",
            ),
            question: t("Welche Orte sind am besten?"),
            options: [
                t("Tür, Sofa, Lampe, Fenster, Regal"),
                t("Vier gleiche Stühle in einer Reihe"),
                t("Fünf Stellen auf derselben Tischplatte"),
            ],
            hint: t("Gute Orte sind auffällig, verschieden und nicht zu nah beieinander."),
            in_locora: Some(t(
                "Im Editor schaltest du „Ort setzen“ ein und tippst auf das Foto. Die Reihenfolge, in der du die Orte setzt, ist deine Route.",
            )),
            action: Action::None,
        },
        Lesson {
            id: "route",
            who: Who::Simo,
            group: Group::Route,
            title: t("Eine Route durchgehen"),
            teaser: t("Immer gleiche Richtung, langsam, Bild abrufen, prüfen."),
            paragraphs: [
                t(
                    "Geh die Orte immer in derselben Richtung ab, wie auf einem festen Spazierweg. So vergisst du keinen.",
                ),
                t(
                    "Geh langsam. Bleib an jedem Ort kurz stehen und frag dich: Was ist hier? Warte, bis das Bild auftaucht.",
                ),
                t(
                    "Dann prüf: Stimmt es? Fehlt ein Bild, schau nach und mach es noch etwas verrückter. Beim nächsten Mal sitzt es.",
                ),
            ],
            example: t(
                "Simo geht los: Fußmatte – eine Kuh, also Milch. Sofa – ein Riesenbrot, also Brot. Badewanne – schwimmende Äpfel, also Äpfel.",
            ),
            exercise: t("Geh Simos Route im Kopf: Fußmatte, Sofa, Badewanne. Was war wo?"),
            question: t("Was lag in Simos Route auf dem Sofa?"),
            options: [t("Ein riesiges Brot"), t("Eine Kuh"), t("Äpfel")],
            hint: t("Geh noch einmal los: erst die Fußmatte, dann das Sofa …"),
            in_locora: Some(t(
                "In Locora tippst du in einem Palast auf „Route ablaufen“. Locora fragt dich Ort für Ort, du deckst auf und sagst, wie gut du es wusstest.",
            )),
            action: Action::Tutorial,
        },
        Lesson {
            id: "review",
            who: Who::Hafiza,
            group: Group::Route,
            title: t("Wiederholen zur richtigen Zeit"),
            teaser: t("Kurz bevor du vergisst – dann hält es am längsten."),
            paragraphs: [
                t(
                    "Was wir lernen, vergessen wir mit der Zeit wieder – am Anfang schnell, später langsamer.",
                ),
                t(
                    "Wiederholst du kurz bevor du etwas vergessen würdest, hält es danach länger. Darum werden die Abstände immer größer: ein Tag, ein paar Tage, Wochen.",
                ),
                t(
                    "Das heißt verteiltes Wiederholen (Spaced Repetition). Locora plant es für dich mit dem Verfahren FSRS: Es merkt sich, wie gut du jeden Ort wusstest.",
                ),
            ],
            example: t(
                "Heute gelernt → morgen wiederholen → in ein paar Tagen → in einer Woche → in einem Monat. Jedes Mal geht es schneller.",
            ),
            exercise: t(
                "Wiederhole jetzt im Kopf Simos Route: Fußmatte, Sofa, Badewanne. Weißt du noch alles?",
            ),
            question: t("Wann wiederholst du am besten?"),
            options: [
                t("In wachsenden Abständen, kurz bevor du es vergessen würdest"),
                t("Zehnmal direkt hintereinander und dann nie wieder"),
                t("Nur wenn gerade Zeit ist"),
            ],
            hint: t("Denk an Hafizas Stoppuhr: Der richtige Zeitpunkt zählt."),
            in_locora: Some(t(
                "Auf der Startseite zeigt Locora, was heute fällig ist; „Weiter ▶“ startet die Wiederholung. In den Einstellungen kannst du Erinnerungen einschalten.",
            )),
            action: Action::Settings,
        },
        Lesson {
            id: "reuse",
            who: Who::Bruno,
            group: Group::Route,
            title: t("Paläste wiederverwenden"),
            teaser: t("Ein Palast für viele Listen – und mehrere Paläste."),
            paragraphs: [
                t(
                    "Einen Palast kannst du immer wieder benutzen. Neue Bilder legen sich über die alten, und die alten verblassen mit der Zeit.",
                ),
                t(
                    "Für Dinge, die du lange behalten willst, nimm einen eigenen Palast. Für kurze Listen wie den Einkauf kannst du immer denselben Palast nehmen.",
                ),
                t(
                    "Mit der Zeit sammelst du mehrere Paläste: Wohnung, Arbeit, Schulweg, Lieblingspark. Gib jedem ein Thema.",
                ),
            ],
            example: t(
                "Bruno: Küche = Einkauf (jede Woche neu), Wohnzimmer = Geburtstage (für immer), Weg zur Arbeit = Vortrag nächste Woche.",
            ),
            exercise: t("Überleg: Welches Thema würdest du welchem Ort geben, den du kennst?"),
            question: t(
                "Du willst dir die Geburtstage deiner Familie für immer merken. Was tust du?",
            ),
            options: [
                t("Ich nehme dafür einen eigenen Palast"),
                t("Ich lege sie in den Einkaufs-Palast, der jede Woche neu belegt wird"),
                t("Ich merke sie mir ohne Bilder"),
            ],
            hint: t("Was lange halten soll, bekommt seinen eigenen Palast."),
            in_locora: Some(t(
                "In Locora kannst du beliebig viele Paläste anlegen und jedem Palast weitere Räume geben.",
            )),
            action: Action::None,
        },
        Lesson {
            id: "mistakes",
            who: Who::Mina,
            group: Group::Route,
            title: t("Häufige Fehler & Tipps"),
            teaser: t("Was am Anfang oft schiefgeht – und wie es klappt."),
            paragraphs: [
                t(
                    "Zu brave Bilder: Ein Apfel auf dem Tisch ist zu normal. Mach ihn riesig, laut oder lustig.",
                ),
                t(
                    "Zu viele oder zu ähnliche Orte: Fang mit 5 bis 10 Orten an. Orte, die sich ähneln, vertauscht man leicht.",
                ),
                t(
                    "Nicht wiederholen: Auch das beste Bild verblasst ohne Wiederholung. Und geh die Route immer in derselben Richtung.",
                ),
            ],
            example: t(
                "Mina erzählt: „Ich habe einmal zwanzig Stühle als Orte genommen. Danach wusste ich nie, auf welchem Stuhl was saß!“",
            ),
            exercise: t(
                "Denk an ein Bild, das du heute gemacht hast. Ist es verrückt genug? Mach es noch ein bisschen verrückter.",
            ),
            question: t("Was ist ein typischer Anfängerfehler?"),
            options: [
                t("Viele gleiche Orte, zum Beispiel lauter Stühle"),
                t("Verrückte Bilder"),
                t("Die Route immer gleich gehen"),
            ],
            hint: t("Zwei der Antworten sind gute Tipps – welche ist der Fehler?"),
            in_locora: None,
            action: Action::None,
        },
        Lesson {
            id: "history",
            who: Who::Simo,
            group: Group::Extra,
            title: t("Geschichte: Simonides"),
            teaser: t("Wie ein Dichter vor 2500 Jahren die Methode fand."),
            paragraphs: [
                t(
                    "Die Loci-Methode ist sehr alt. Ihre Erfindung wird dem griechischen Dichter Simonides von Keos zugeschrieben, der vor etwa 2500 Jahren lebte.",
                ),
                t(
                    "Der Legende nach trug er bei einem Festmahl ein Gedicht vor und wurde kurz hinausgerufen. Da stürzte die Decke ein. Die Gäste waren nicht mehr zu erkennen – aber Simonides wusste noch, wer wo gesessen hatte.",
                ),
                t(
                    "So soll er entdeckt haben: Orte in fester Reihenfolge helfen dem Gedächtnis. Römische Redner wie Cicero und Quintilian beschrieben die Methode später, um lange Reden frei zu halten. Simo ist nach Simonides benannt.",
                ),
            ],
            example: t(
                "Auch Simos Freunde tragen Namen aus der Geschichte: Mina nach Mnemosyne, der griechischen Göttin der Erinnerung; Cami nach Giulio Camillo und seinem „Gedächtnistheater“; Bruno nach Giordano Bruno, der über die Gedächtniskunst schrieb. Hafiza heißt auf Türkisch „Gedächtnis“.",
            ),
            exercise: t(
                "Simonides wusste, wer wo saß. Denk an dein letztes Essen mit anderen: Wer saß wo?",
            ),
            question: t("Wem wird die Erfindung der Loci-Methode zugeschrieben?"),
            options: [
                t("Simonides von Keos"),
                t("Albert Einstein"),
                t("Leonardo da Vinci"),
            ],
            hint: t("Ein Tipp: Simo trägt seinen Namen."),
            in_locora: None,
            action: Action::None,
        },
        Lesson {
            id: "research",
            who: Who::Mina,
            group: Group::Extra,
            title: t("Was die Forschung sagt"),
            teaser: t("Was gut belegt ist – ohne Übertreibung."),
            paragraphs: [
                t(
                    "Eine Studie von Maguire und Kollegen (2003) untersuchte Gedächtnis-Meister. Ihre Gehirne waren im Aufbau nicht besonders – aber fast alle nutzten eine Orts-Strategie wie die Loci-Methode.",
                ),
                t(
                    "Dresler und Kollegen (2017, Fachzeitschrift Neuron) ließen Menschen ohne Vorkenntnisse sechs Wochen lang mit der Loci-Methode üben. Danach merkten sie sich deutlich mehr Wörter, auch noch Monate später, und die Vernetzung im Gehirn veränderte sich in Richtung der Gedächtnis-Meister.",
                ),
                t(
                    "Wichtig: Die Methode hilft vor allem bei Listen und Reihenfolgen. Sie ersetzt nicht das Verstehen – und sie wirkt durch Üben, nicht von allein.",
                ),
            ],
            example: t(
                "In der Studie von 2017 übten die Teilnehmenden etwa eine halbe Stunde am Tag.",
            ),
            exercise: t(
                "Überleg: Bei welcher Liste im Alltag würde dir ein Gedächtnispalast am meisten helfen?",
            ),
            question: t("Was zeigte die Studie von Dresler und Kollegen (2017)?"),
            options: [
                t("Sechs Wochen Üben mit der Loci-Methode verbesserten das Merken deutlich"),
                t("Die Methode wirkt sofort, ganz ohne Üben"),
                t("Nur Menschen mit einem besonderen Gehirn können sie lernen"),
            ],
            hint: t("Mina betont: Es wirkt durch Üben – und jeder kann es lernen."),
            in_locora: None,
            action: Action::None,
        },
        Lesson {
            id: "sport",
            who: Who::Hafiza,
            group: Group::Extra,
            title: t("Gedächtnissport"),
            teaser: t("Wettkämpfe im Merken – mit genau dieser Methode."),
            paragraphs: [
                t(
                    "Es gibt Wettkämpfe im Merken, zum Beispiel Meisterschaften im Gedächtnissport. Dort merkt man sich Zahlen, Spielkarten, Namen und Gesichter auf Zeit.",
                ),
                t(
                    "Fast alle Spitzenleute im Gedächtnissport nutzen Gedächtnispaläste – also genau das, was du hier lernst – dazu feste Bilder für Zahlen und Karten.",
                ),
                t(
                    "Du musst an keinem Wettkampf teilnehmen. Aber er zeigt: Ein gutes Gedächtnis ist zum großen Teil Technik und Übung.",
                ),
            ],
            example: t(
                "Eine bekannte Disziplin: ein gemischtes Kartenspiel so schnell wie möglich merken. Die Besten schaffen das in weniger als 20 Sekunden.",
            ),
            exercise: t(
                "Merk dir in 20 Sekunden: Hut, Fisch, Gitarre, Kerze, Wolke – leg sie an fünf Orte in deinem Zimmer.",
            ),
            question: t("Was nutzen die meisten Gedächtnissportler?"),
            options: [
                t("Gedächtnispaläste und feste Bilder"),
                t("Besonders viel Kaffee"),
                t("Gar keine Technik"),
            ],
            hint: t("Es ist genau die Methode, die du hier lernst."),
            in_locora: None,
            action: Action::None,
        },
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_lessons_are_the_ones_the_backend_knows() {
        let ids: Vec<&str> = super::lessons().iter().map(|lesson| lesson.id).collect();
        assert_eq!(ids, locora_app_api::LEARN_LESSONS);
    }
}
