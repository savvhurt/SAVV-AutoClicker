//! Interface languages and a tiny compile-time translation table.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Ru,
    De,
    Fr,
    Es,
    Pt,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    Auto,
    En,
    Ru,
    De,
    Fr,
    Es,
    Pt,
}

impl Language {
    pub const ALL: [Self; 7] = [
        Self::Auto,
        Self::En,
        Self::Ru,
        Self::De,
        Self::Fr,
        Self::Es,
        Self::Pt,
    ];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "auto" | "automatic" | "системный" => Some(Self::Auto),
            "en" | "eng" | "english" | "английский" => Some(Self::En),
            "ru" | "rus" | "russian" | "русский" => Some(Self::Ru),
            "de" | "deu" | "german" | "deutsch" => Some(Self::De),
            "fr" | "fra" | "french" | "français" | "francais" => Some(Self::Fr),
            "es" | "spa" | "spanish" | "español" | "espanol" => Some(Self::Es),
            "pt" | "por" | "portuguese" | "português" | "portugues" => Some(Self::Pt),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::En => "en",
            Self::Ru => "ru",
            Self::De => "de",
            Self::Fr => "fr",
            Self::Es => "es",
            Self::Pt => "pt",
        }
    }

    pub fn native_label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::En => "English",
            Self::Ru => "Русский",
            Self::De => "Deutsch",
            Self::Fr => "Français",
            Self::Es => "Español",
            Self::Pt => "Português",
        }
    }

    pub fn resolve(self) -> Lang {
        match self {
            Self::Auto => detect(),
            Self::En => Lang::En,
            Self::Ru => Lang::Ru,
            Self::De => Lang::De,
            Self::Fr => Lang::Fr,
            Self::Es => Lang::Es,
            Self::Pt => Lang::Pt,
        }
    }
}

fn detect() -> Lang {
    sys_locale::get_locale()
        .as_deref()
        .and_then(lang_from_locale)
        .unwrap_or(Lang::En)
}

fn lang_from_locale(locale: &str) -> Option<Lang> {
    let prefix = locale
        .split(['_', '-', '.', '@'])
        .next()?
        .to_ascii_lowercase();
    match prefix.as_str() {
        "en" => Some(Lang::En),
        "ru" => Some(Lang::Ru),
        "de" => Some(Lang::De),
        "fr" => Some(Lang::Fr),
        "es" => Some(Lang::Es),
        "pt" => Some(Lang::Pt),
        _ => None,
    }
}

macro_rules! define_strings {
    ($( $variant:ident : [$en:literal, $ru:literal, $de:literal, $fr:literal, $es:literal, $pt:literal] ),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Key {
            $($variant),*
        }

        pub fn tr(lang: Lang, key: Key) -> &'static str {
            match key {
                $(
                    Key::$variant => match lang {
                        Lang::En => $en,
                        Lang::Ru => $ru,
                        Lang::De => $de,
                        Lang::Fr => $fr,
                        Lang::Es => $es,
                        Lang::Pt => $pt,
                    }
                ),*
            }
        }
    };
}

define_strings! {
    ToggleTheme: [
        "Switch light and dark theme",
        "Переключить светлую и тёмную тему",
        "Helles und dunkles Design umschalten",
        "Basculer entre le thème clair et sombre",
        "Cambiar entre tema claro y oscuro",
        "Alternar entre tema claro e escuro"
    ],
    ThemeLight: ["LIGHT", "СВЕТЛАЯ", "HELL", "CLAIR", "CLARO", "CLARO"],
    ThemeDark: ["DARK", "ТЁМНАЯ", "DUNKEL", "SOMBRE", "OSCURO", "ESCURO"],
    ThemeNotice: ["Theme: {}.", "Тема: {}.", "Design: {}.", "Thème : {}.", "Tema: {}.", "Tema: {}."],
    StatusStarting: ["STARTING", "ЗАПУСК", "START", "DÉMARRAGE", "INICIANDO", "INICIANDO"],
    StatusStopping: ["STOPPING", "ОСТАНОВКА", "STOPPEN", "ARRÊT", "DETENIENDO", "PARANDO"],
    StatusOffline: ["OFFLINE", "НЕТ СВЯЗИ", "OFFLINE", "HORS LIGNE", "SIN CONEXIÓN", "SEM CONEXÃO"],
    StatusDelay: ["DELAY", "ЗАДЕРЖКА", "WARTEN", "DÉLAI", "RETRASO", "ATRASO"],
    StatusActive: ["ACTIVE", "АКТИВЕН", "AKTIV", "ACTIF", "ACTIVO", "ATIVO"],
    StatusIdle: ["IDLE", "ОЖИДАНИЕ", "BEREIT", "EN ATTENTE", "EN ESPERA", "OCIOSO"],
    HeroStarting: ["Starting…", "Запуск…", "Startet…", "Démarrage…", "Iniciando…", "Iniciando…"],
    HeroStopping: ["Stopping…", "Остановка…", "Wird gestoppt…", "Arrêt…", "Deteniendo…", "Parando…"],
    HeroWaitDaemon: [
        "Waiting for the daemon to confirm.",
        "Ждём подтверждения от демона.",
        "Warte auf die Bestätigung des Daemons.",
        "En attente de la confirmation du démon.",
        "Esperando confirmación del daemon.",
        "Aguardando confirmação do daemon."
    ],
    HeroDelayTitle: [
        "Starting in {:.1} s",
        "Старт через {:.1} с",
        "Start in {:.1} s",
        "Démarrage dans {:.1} s",
        "Inicio en {:.1} s",
        "Início em {:.1} s"
    ],
    HeroDelayText: [
        "Clicks begin after the delay. Stopping cancels the start.",
        "Клики начнутся после задержки. Выключение отменит запуск.",
        "Die Klicks beginnen nach der Verzögerung. Stoppen bricht den Start ab.",
        "Les clics commencent après le délai. Arrêter annule le démarrage.",
        "Los clics comienzan tras el retraso. Detener cancela el inicio.",
        "Os cliques começam após o atraso. Parar cancela o início."
    ],
    HeroHoldTitle: ["Holding", "Удержание", "Halten", "Maintien", "Manteniendo", "Segurando"],
    HeroHoldText: [
        "Clicks run while the hotkey is held.",
        "Клики идут, пока горячая клавиша зажата.",
        "Klicks laufen, solange die Tastenkombination gehalten wird.",
        "Les clics continuent tant que la touche est maintenue.",
        "Los clics siguen mientras se mantiene la tecla.",
        "Os cliques continuam enquanto a tecla é mantida."
    ],
    HeroSessionProgress: [
        "{} of {} session clicks.",
        "{} из {} кликов сессии.",
        "{} von {} Klicks dieser Sitzung.",
        "{} clics sur {} pour cette session.",
        "{} de {} clics de la sesión.",
        "{} de {} cliques da sessão."
    ],
    HeroTargetClicks: [
        "Pulses go to the fixed point.",
        "Импульсы уходят в заданную точку.",
        "Impulse gehen an den festen Punkt.",
        "Les impulsions vont au point fixe.",
        "Los pulsos van al punto fijo.",
        "Os pulsos vão para o ponto fixo."
    ],
    HeroCursorClicks: [
        "Pulses go to the current cursor position.",
        "Импульсы уходят в текущую позицию курсора.",
        "Impulse gehen an die aktuelle Cursorposition.",
        "Les impulsions vont à la position actuelle du curseur.",
        "Los pulsos van a la posición actual del cursor.",
        "Os pulsos vão para a posição atual do cursor."
    ],
    HeroClicking: ["Clicking", "Клики идут", "Klicks laufen", "Clics en cours", "Clics en curso", "Cliques em andamento"],
    HeroLimitDone: ["Limit reached", "Лимит выполнен", "Limit erreicht", "Limite atteinte", "Límite alcanzado", "Limite atingido"],
    HeroLimitText: [
        "{} session clicks. Start again or change the limit.",
        "{} кликов сессии. Запустите снова или измените лимит.",
        "{} Klicks in dieser Sitzung. Erneut starten oder das Limit ändern.",
        "{} clics pour cette session. Relancez ou modifiez la limite.",
        "{} clics de la sesión. Inicia de nuevo o cambia el límite.",
        "{} cliques da sessão. Inicie novamente ou altere o limite."
    ],
    HeroStopped: ["Clicks stopped", "Клики остановлены", "Klicks gestoppt", "Clics arrêtés", "Clics detenidos", "Cliques parados"],
    HeroPressStart: [
        "Press the button to start.",
        "Нажмите кнопку, чтобы начать.",
        "Drücke die Taste, um zu starten.",
        "Appuyez sur le bouton pour démarrer.",
        "Pulsa el botón para empezar.",
        "Pressione o botão para começar."
    ],
    HeroDaemonAuto: [
        "The daemon will start automatically.",
        "Демон запустится автоматически.",
        "Der Daemon startet automatisch.",
        "Le démon démarrera automatiquement.",
        "El daemon se iniciará automáticamente.",
        "O daemon iniciará automaticamente."
    ],
    MetricCps: ["ACTUAL RATE", "ФАКТ. СКОРОСТЬ", "ECHTE RATE", "DÉBIT RÉEL", "TASA REAL", "TAXA REAL"],
    MetricClicks: ["CLICKS", "КЛИКОВ", "KLICKS", "CLICS", "CLICS", "CLIQUES"],
    MetricSession: ["SESSION", "СЕССИЯ", "SITZUNG", "SESSION", "SESIÓN", "SESSÃO"],
    UnitPieces: ["PCS", "ШТ", "STK", "PCS", "UDS", "UN"],
    UnitMs: ["MS", "МС", "MS", "MS", "MS", "MS"],
    Enabled: ["ON", "ВКЛ", "EIN", "ACTIF", "ACTIVADO", "ATIVADO"],
    Disabled: ["OFF", "ВЫКЛ", "AUS", "INACTIF", "DESACTIVADO", "DESATIVADO"],
    NoticeError: ["ERROR", "ОШИБКА", "FEHLER", "ERREUR", "ERROR", "ERRO"],
    NoticeSystem: ["SYSTEM", "СИСТЕМА", "SYSTEM", "SYSTÈME", "SISTEMA", "SISTEMA"],
    NoticeHotkey: ["HOTKEY", "ХОТКЕЙ", "TASTE", "RACCOURCI", "ATAJO", "ATALHO"],
    NoticeStart: ["START", "СТАРТ", "START", "DÉMARRAGE", "INICIO", "INÍCIO"],
    NoticeLimit: ["LIMIT", "ЛИМИТ", "LIMIT", "LIMITE", "LÍMITE", "LIMITE"],
    NoticeDaemon: ["DAEMON", "ДЕМОН", "DAEMON", "DÉMON", "DAEMON", "DAEMON"],
    NoticeReady: ["READY", "ГОТОВО", "BEREIT", "PRÊT", "LISTO", "PRONTO"],
    NoticeStartsIn: [
        "Clicks start in {:.1} s.",
        "Клики начнутся через {:.1} с.",
        "Klicks starten in {:.1} s.",
        "Les clics commencent dans {:.1} s.",
        "Los clics empiezan en {:.1} s.",
        "Os cliques começam em {:.1} s."
    ],
    NoticeLimitDone: [
        "{} session clicks completed.",
        "Выполнено {} кликов сессии.",
        "{} Klicks dieser Sitzung abgeschlossen.",
        "{} clics de la session effectués.",
        "{} clics de la sesión completados.",
        "{} cliques da sessão concluídos."
    ],
    NoticeDaemonDown: [
        "Not running — starts together with clicking.",
        "Не запущен — стартует вместе с кликами.",
        "Nicht aktiv — startet zusammen mit den Klicks.",
        "Inactif — démarre avec les clics.",
        "No está activo — se inicia con los clics.",
        "Inativo — inicia junto com os cliques."
    ],
    NoticeAutoApply: [
        "Changes are applied automatically.",
        "Изменения применяются автоматически.",
        "Änderungen werden automatisch übernommen.",
        "Les modifications sont appliquées automatiquement.",
        "Los cambios se aplican automáticamente.",
        "As alterações são aplicadas automaticamente."
    ],
    TabClicks: ["CLICKS", "КЛИКИ", "KLICKEN", "CLICS", "CLICS", "CLIQUES"],
    TabMode: ["MODE", "РЕЖИМ", "MODUS", "MODE", "MODO", "MODO"],
    TabHotkeys: ["HOTKEYS", "КЛАВИШИ", "TASTEN", "TOUCHES", "TECLAS", "ATALHOS"],
    TabSystem: ["SYSTEM", "СИСТЕМА", "SYSTEM", "SYSTÈME", "SISTEMA", "SISTEMA"],
    RowSpeed: ["Speed", "Скорость", "Geschwindigkeit", "Vitesse", "Velocidad", "Velocidade"],
    HintSpeed: [
        "Clicks per second",
        "Частота кликов в секунду",
        "Klicks pro Sekunde",
        "Clics par seconde",
        "Clics por segundo",
        "Cliques por segundo"
    ],
    RowButton: ["Button", "Кнопка", "Taste", "Bouton", "Botón", "Botão"],
    HintButton: [
        "Which mouse button to press",
        "Какую кнопку мыши нажимать",
        "Welche Maustaste gedrückt wird",
        "Quel bouton de souris presser",
        "Qué botón del ratón pulsar",
        "Qual botão do mouse pressionar"
    ],
    ButtonLeft: ["LEFT", "ЛЕВАЯ", "LINKS", "GAUCHE", "IZQ.", "ESQ."],
    ButtonRight: ["RIGHT", "ПРАВАЯ", "RECHTS", "DROITE", "DER.", "DIR."],
    ButtonMiddle: ["MIDDLE", "СРЕДНЯЯ", "MITTE", "MILIEU", "MEDIO", "MEIO"],
    RowHold: ["Hold", "Удержание", "Haltezeit", "Maintien", "Mantener", "Duração"],
    HintHold: [
        "Press duration; limited by the interval",
        "Длительность нажатия; ограничивается периодом",
        "Druckdauer; durch das Intervall begrenzt",
        "Durée d'appui ; limitée par l'intervalle",
        "Duración de pulsación; limitada por el intervalo",
        "Duração do pressionamento; limitada pelo intervalo"
    ],
    RowJitter: ["Jitter", "Джиттер", "Jitter", "Variation", "Variación", "Variação"],
    HintJitter: [
        "Random interval spread",
        "Случайный разброс интервала",
        "Zufällige Streuung des Intervalls",
        "Variation aléatoire de l'intervalle",
        "Variación aleatoria del intervalo",
        "Variação aleatória do intervalo"
    ],
    RowLimit: ["Limit", "Лимит", "Limit", "Limite", "Límite", "Limite"],
    HintLimit: [
        "Clicks per session. 0 means unlimited",
        "Сколько кликов сделать за сессию. 0 — без ограничения",
        "Klicks pro Sitzung. 0 bedeutet unbegrenzt",
        "Clics par session. 0 signifie illimité",
        "Clics por sesión. 0 significa ilimitado",
        "Cliques por sessão. 0 significa ilimitado"
    ],
    RowDelay: ["Delay", "Задержка", "Verzögerung", "Délai", "Retraso", "Atraso"],
    HintDelay: [
        "Pause before a new session, 0 - 10000 ms",
        "Пауза перед началом новой сессии, 0 - 10000 мс",
        "Pause vor einer neuen Sitzung, 0 - 10000 ms",
        "Pause avant une nouvelle session, 0 - 10000 ms",
        "Pausa antes de una nueva sesión, 0 - 10000 ms",
        "Pausa antes de uma nova sessão, 0 - 10000 ms"
    ],
    HintHoldClamped: [
        "Effective hold {} ms: the rate does not allow a longer press.",
        "Фактическое удержание {} мс: частота не даёт удержать дольше.",
        "Tatsächliche Haltezeit {} ms: die Rate erlaubt keinen längeren Druck.",
        "Maintien effectif {} ms : la fréquence ne permet pas plus long.",
        "Mantenimiento efectivo {} ms: la tasa no permite pulsar más tiempo.",
        "Duração efetiva {} ms: a taxa não permite pressionar por mais tempo."
    ],
    HintLimitOn: [
        "Limit of {} clicks per session; 0 disables the limit.",
        "Лимит {} кликов за сессию; 0 отключает лимит.",
        "Limit von {} Klicks pro Sitzung; 0 hebt das Limit auf.",
        "Limite de {} clics par session ; 0 désactive la limite.",
        "Límite de {} clics por sesión; 0 desactiva el límite.",
        "Limite de {} cliques por sessão; 0 desativa o limite."
    ],
    HintLimitOff: [
        "Limit 0 means unlimited. The delay applies only at start.",
        "Лимит 0 — без ограничения. Задержка действует только на старте.",
        "Limit 0 bedeutet unbegrenzt. Die Verzögerung gilt nur beim Start.",
        "Limite 0 signifie illimité. Le délai ne s'applique qu'au démarrage.",
        "Límite 0 significa ilimitado. El retraso solo se aplica al inicio.",
        "Limite 0 significa ilimitado. O atraso só se aplica no início."
    ],
    RowTarget: ["Target", "Цель", "Ziel", "Cible", "Objetivo", "Alvo"],
    HintTarget: [
        "Where the clicks are sent",
        "Куда отправлять клики",
        "Wohin die Klicks gehen",
        "Où envoyer les clics",
        "Adónde van los clics",
        "Para onde vão os cliques"
    ],
    TargetCursor: ["CURSOR", "КУРСОР", "CURSOR", "CURSEUR", "CURSOR", "CURSOR"],
    TargetPoint: ["POINT", "ТОЧКА", "PUNKT", "POINT", "PUNTO", "PONTO"],
    RowCoords: ["Coordinates", "Координаты", "Koordinaten", "Coordonnées", "Coordenadas", "Coordenadas"],
    HintCoords: [
        "X/Y pixels from the top-left corner",
        "X/Y в пикселях от левого верхнего угла",
        "X/Y-Pixel ab der linken oberen Ecke",
        "Pixels X/Y depuis le coin supérieur gauche",
        "Píxeles X/Y desde la esquina superior izquierda",
        "Pixels X/Y a partir do canto superior esquerdo"
    ],
    PickCurrent: [
        "USE CURRENT",
        "ВЗЯТЬ ТЕКУЩУЮ",
        "AKTUELLE NEHMEN",
        "PRENDRE ACTUELLE",
        "USAR ACTUAL",
        "USAR ATUAL"
    ],
    CurrentPicked: [
        "Current position: {}, {}.",
        "Текущая позиция: {}, {}.",
        "Aktuelle Position: {}, {}.",
        "Position actuelle : {}, {}.",
        "Posición actual: {}, {}.",
        "Posição atual: {}, {}."
    ],
    TargetWayland: [
        "Fixed-point clicking is not available on Wayland. Use X11 or Windows.",
        "Клик в точку недоступен в Wayland. Используйте X11 или Windows.",
        "Klicken an einem festen Punkt ist unter Wayland nicht verfügbar. X11 oder Windows verwenden.",
        "Le clic à un point fixe n'est pas disponible sous Wayland. Utilisez X11 ou Windows.",
        "El clic en un punto fijo no está disponible en Wayland. Usa X11 o Windows.",
        "O clique em ponto fixo não está disponível no Wayland. Use X11 ou Windows."
    ],
    TargetNoDisplay: [
        "Fixed-point clicking requires an X11 display.",
        "Для клика в точку нужен X11-дисплей.",
        "Klicken an einem festen Punkt benötigt ein X11-Display.",
        "Le clic à un point fixe nécessite un affichage X11.",
        "El clic en un punto fijo requiere una pantalla X11.",
        "O clique em ponto fixo requer um display X11."
    ],
    PickFailed: [
        "Could not read the cursor position: {}",
        "Не удалось получить позицию курсора: {}",
        "Cursorposition konnte nicht gelesen werden: {}",
        "Impossible de lire la position du curseur : {}",
        "No se pudo leer la posición del cursor: {}",
        "Não foi possível ler a posição do cursor: {}"
    ],
    RowMulti: ["Multi-click", "Мультиклик", "Mehrfachklick", "Multi-clic", "Multiclic", "Multiclique"],
    HintMulti: [
        "Clicks sent in one pulse",
        "Сколько кликов за один импульс",
        "Klicks pro Impuls",
        "Clics par impulsion",
        "Clics por impulso",
        "Cliques por impulso"
    ],
    MultiOne: ["SINGLE", "ОДИН", "EINZEL", "SIMPLE", "UNO", "ÚNICO"],
    MultiDouble: ["DOUBLE", "ДВОЙНОЙ", "DOPPEL", "DOUBLE", "DOBLE", "DUPLO"],
    MultiTriple: ["TRIPLE", "ТРОЙНОЙ", "DREIFACH", "TRIPLE", "TRIPLE", "TRIPLO"],
    MultiHint: [
        "A 40 ms pause separates clicks inside a pulse. The limit counts every click.",
        "Между кликами внутри импульса пауза 40 мс. Лимит считает каждый клик.",
        "Zwischen den Klicks eines Impulses liegen 40 ms. Das Limit zählt jeden Klick.",
        "Une pause de 40 ms sépare les clics d'une impulsion. La limite compte chaque clic.",
        "Una pausa de 40 ms separa los clics de un impulso. El límite cuenta cada clic.",
        "Uma pausa de 40 ms separa os cliques de um impulso. O limite conta cada clique."
    ],
    RowToggleHotkey: ["Toggle", "Переключение", "Umschalten", "Basculer", "Alternar", "Alternar"],
    HintToggleHotkey: [
        "Global on/off for clicking",
        "Глобальное включение и выключение кликов",
        "Globales Ein-/Ausschalten der Klicks",
        "Activation/désactivation globale des clics",
        "Activación/desactivación global de los clics",
        "Ativação/desativação global dos cliques"
    ],
    RowHoldHotkey: ["Hold", "Удержание", "Halten", "Maintien", "Mantener", "Segurar"],
    HintHoldHotkey: [
        "Clicks run while the key is held",
        "Клики идут, пока клавиша зажата",
        "Klicks laufen, solange die Taste gehalten wird",
        "Les clics continuent tant que la touche est maintenue",
        "Los clics siguen mientras se mantiene la tecla",
        "Os cliques continuam enquanto a tecla é mantida"
    ],
    Clear: ["CLEAR", "ОЧИСТИТЬ", "LÖSCHEN", "EFFACER", "BORRAR", "LIMPAR"],
    Registered: [
        "Currently registered: {}",
        "Сейчас зарегистрирована: {}",
        "Derzeit registriert: {}",
        "Actuellement enregistré : {}",
        "Registrado actualmente: {}",
        "Registrado atualmente: {}"
    ],
    RecordWaiting: [
        "WAITING FOR KEY",
        "ЖДЁМ КЛАВИШУ",
        "WARTE AUF TASTE",
        "EN ATTENTE D'UNE TOUCHE",
        "ESPERANDO TECLA",
        "AGUARDANDO TECLA"
    ],
    NotSet: ["NOT SET", "НЕ ЗАДАНА", "NICHT GESETZT", "NON DÉFINI", "NO DEFINIDA", "NÃO DEFINIDA"],
    RecordHint: [
        "Press any key for “{}”. Click the field again to cancel.",
        "Нажмите любую клавишу для «{}». Повторный клик по полю — отмена.",
        "Drücke eine beliebige Taste für „{}“. Erneuter Klick auf das Feld bricht ab.",
        "Appuyez sur une touche pour « {} ». Cliquez à nouveau sur le champ pour annuler.",
        "Pulsa cualquier tecla para «{}». Haz clic de nuevo en el campo para cancelar.",
        "Pressione qualquer tecla para “{}”. Clique novamente no campo para cancelar."
    ],
    RecordTargetToggle: ["toggle", "переключения", "Umschalten", "basculer", "alternar", "alternar"],
    RecordTargetHold: ["hold", "удержания", "Halten", "maintien", "mantener", "segurar"],
    HotkeyPaused: [
        "Hotkey registration is paused.",
        "Регистрация горячих клавиш приостановлена.",
        "Die Tastenregistrierung ist pausiert.",
        "L'enregistrement des raccourcis est en pause.",
        "El registro de atajos está en pausa.",
        "O registro de atalhos está pausado."
    ],
    HotkeyHint: [
        "A key without modifiers also works; F12 may be reserved by Windows.",
        "Клавиша без модификаторов тоже работает; F12 может быть занята Windows.",
        "Eine Taste ohne Modifikatoren funktioniert auch; F12 kann von Windows belegt sein.",
        "Une touche sans modificateur fonctionne aussi ; F12 peut être réservée par Windows.",
        "Una tecla sin modificadores también funciona; F12 puede estar reservada por Windows.",
        "Uma tecla sem modificadores também funciona; F12 pode estar reservada pelo Windows."
    ],
    RowGlobal: ["Global", "Глобальная", "Global", "Global", "Global", "Global"],
    HintGlobal: [
        "The compositor defines the shortcut",
        "Комбинацию задаёт композитор",
        "Der Compositor legt das Kürzel fest",
        "Le compositeur définit le raccourci",
        "El compositor define el atajo",
        "O compositor define o atalho"
    ],
    CopyBind: [
        "Click to copy the bind",
        "Нажмите, чтобы скопировать bind",
        "Klicken, um den Bind zu kopieren",
        "Cliquez pour copier le bind",
        "Haz clic para copiar el bind",
        "Clique para copiar o bind"
    ],
    BindCopied: [
        "Bind copied to the clipboard.",
        "Bind скопирован в буфер обмена.",
        "Bind in die Zwischenablage kopiert.",
        "Bind copié dans le presse-papiers.",
        "Bind copiado al portapapeles.",
        "Bind copiado para a área de transferência."
    ],
    BindHint: [
        "The compositor owns the global shortcut. Click MOD+ALT+C to copy a ready-to-use niri bind.",
        "Глобальную комбинацию задаёт композитор. Нажмите MOD+ALT+C, чтобы скопировать готовый bind для niri.",
        "Der Compositor verwaltet das globale Tastenkürzel. Klicke auf MOD+ALT+C, um einen fertigen niri-Bind zu kopieren.",
        "Le compositeur gère le raccourci global. Cliquez sur MOD+ALT+C pour copier un bind niri prêt à l'emploi.",
        "El compositor gestiona el atajo global. Haz clic en MOD+ALT+C para copiar un bind de niri listo.",
        "O compositor gerencia o atalho global. Clique em MOD+ALT+C para copiar um bind do niri pronto."
    ],
    HoldWindowsOnly: [
        "Hold-to-click is available on Windows only.",
        "Удержание доступно только в Windows.",
        "Halten ist nur unter Windows verfügbar.",
        "Le maintien n'est disponible que sous Windows.",
        "El mantenimiento solo está disponible en Windows.",
        "O modo segurar está disponível apenas no Windows."
    ],
    BindingFailed: [
        "Could not assign the key: {}. Supported keys: A-Z, 0-9, F1-F24, Space, Tab, Enter, Esc, arrows and punctuation.",
        "Не удалось назначить клавишу: {}. Поддерживаются A-Z, 0-9, F1-F24, Space, Tab, Enter, Esc, стрелки и знаки.",
        "Taste konnte nicht zugewiesen werden: {}. Unterstützt: A-Z, 0-9, F1-F24, Leertaste, Tab, Enter, Esc, Pfeile und Satzzeichen.",
        "Impossible d'assigner la touche : {}. Touches prises en charge : A-Z, 0-9, F1-F24, Espace, Tab, Entrée, Échap, flèches et ponctuation.",
        "No se pudo asignar la tecla: {}. Teclas compatibles: A-Z, 0-9, F1-F24, Espacio, Tab, Intro, Esc, flechas y signos.",
        "Não foi possível atribuir a tecla: {}. Teclas suportadas: A-Z, 0-9, F1-F24, Espaço, Tab, Enter, Esc, setas e pontuação."
    ],
    KeyAlreadyHold: [
        "This key is already assigned to hold.",
        "Эта клавиша уже назначена для удержания.",
        "Diese Taste ist bereits für Halten belegt.",
        "Cette touche est déjà assignée au maintien.",
        "Esta tecla ya está asignada a mantener.",
        "Esta tecla já está atribuída a segurar."
    ],
    KeyAlreadyToggle: [
        "This key is already assigned to toggle.",
        "Эта клавиша уже назначена для переключения.",
        "Diese Taste ist bereits für Umschalten belegt.",
        "Cette touche est déjà assignée au basculement.",
        "Esta tecla ya está asignada a alternar.",
        "Esta tecla já está atribuída a alternar."
    ],
    BoundNotice: ["{}: {}.", "{}: {}.", "{}: {}.", "{} : {}.", "{}: {}.", "{}: {}."],
    HoldDisabled: [
        "Hold key disabled.",
        "Клавиша удержания отключена.",
        "Haltetaste deaktiviert.",
        "Touche de maintien désactivée.",
        "Tecla de mantener desactivada.",
        "Tecla de segurar desativada."
    ],
    RowLanguage: ["Language", "Язык", "Sprache", "Langue", "Idioma", "Idioma"],
    HintLanguage: [
        "Interface language; Auto follows the system",
        "Язык интерфейса; «Авто» берёт системный",
        "Sprache der Oberfläche; Automatisch folgt dem System",
        "Langue de l'interface ; Auto suit le système",
        "Idioma de la interfaz; Auto sigue al sistema",
        "Idioma da interface; Auto segue o sistema"
    ],
    RowTheme: ["Theme", "Тема", "Design", "Thème", "Tema", "Tema"],
    HintTheme: [
        "Light or dark appearance",
        "Светлое или тёмное оформление",
        "Helles oder dunkles Erscheinungsbild",
        "Apparence claire ou sombre",
        "Apariencia clara u oscura",
        "Aparência clara ou escura"
    ],
    RowAutostart: ["Start at login", "Запускать при входе", "Beim Anmelden starten", "Lancer à la connexion", "Iniciar al entrar", "Iniciar no login"],
    HintAutostart: [
        "The background daemon starts with the Linux session",
        "Фоновый демон стартует вместе с сеансом Linux",
        "Der Hintergrund-Daemon startet mit der Linux-Sitzung",
        "Le démon d'arrière-plan démarre avec la session Linux",
        "El daemon en segundo plano se inicia con la sesión de Linux",
        "O daemon em segundo plano inicia com a sessão do Linux"
    ],
    AutostartUnavailable: [
        "Autostart is available on Linux only.",
        "Автозапуск доступен только в Linux.",
        "Autostart ist nur unter Linux verfügbar.",
        "Le démarrage automatique n'est disponible que sous Linux.",
        "El inicio automático solo está disponible en Linux.",
        "A inicialização automática está disponível apenas no Linux."
    ],
    AutostartOn: [
        "Autostart enabled.",
        "Автозапуск включён.",
        "Autostart aktiviert.",
        "Démarrage automatique activé.",
        "Inicio automático activado.",
        "Inicialização automática ativada."
    ],
    AutostartOff: [
        "Autostart disabled.",
        "Автозапуск выключен.",
        "Autostart deaktiviert.",
        "Démarrage automatique désactivé.",
        "Inicio automático desactivado.",
        "Inicialização automática desativada."
    ],
    AutostartFailed: [
        "Could not change autostart: {}",
        "Не удалось изменить автозапуск: {}",
        "Autostart konnte nicht geändert werden: {}",
        "Impossible de modifier le démarrage automatique : {}",
        "No se pudo cambiar el inicio automático: {}",
        "Não foi possível alterar a inicialização automática: {}"
    ],
    AutostartHint: [
        "Adds a desktop entry for the current executable. Re-enable it after moving the binary.",
        "Добавляет запись для текущего исполняемого файла. После переноса бинарника включите заново.",
        "Fügt einen Eintrag für die aktuelle ausführbare Datei hinzu. Nach dem Verschieben erneut aktivieren.",
        "Ajoute une entrée pour l'exécutable actuel. Réactivez après avoir déplacé le binaire.",
        "Añade una entrada para el ejecutable actual. Vuelve a activarlo si mueves el binario.",
        "Adiciona uma entrada para o executável atual. Reative após mover o binário."
    ],
    DaemonRunning: ["DAEMON RUNNING", "ДЕМОН РАБОТАЕТ", "DAEMON LÄUFT", "DÉMON ACTIF", "DAEMON ACTIVO", "DAEMON ATIVO"],
    DaemonStopped: ["DAEMON NOT RUNNING", "ДЕМОН НЕ ЗАПУЩЕН", "DAEMON NICHT AKTIV", "DÉMON INACTIF", "DAEMON INACTIVO", "DAEMON INATIVO"],
    QuitDaemon: ["STOP DAEMON", "ЗАВЕРШИТЬ ДЕМОН", "DAEMON BEENDEN", "ARRÊTER LE DÉMON", "DETENER DAEMON", "ENCERRAR DAEMON"],
    QuitTitle: ["SHUTDOWN", "ЗАВЕРШЕНИЕ РАБОТЫ", "BEENDEN", "ARRÊT", "APAGADO", "ENCERRAMENTO"],
    QuitQuestion: ["Stop the daemon?", "Завершить демон?", "Daemon beenden?", "Arrêter le démon ?", "¿Detener el daemon?", "Encerrar o daemon?"],
    QuitText: [
        "Clicks will stop and the daemon will exit.",
        "Клики будут остановлены, демон завершит работу.",
        "Die Klicks werden gestoppt und der Daemon beendet.",
        "Les clics s'arrêteront et le démon quittera.",
        "Los clics se detendrán y el daemon saldrá.",
        "Os cliques pararão e o daemon será encerrado."
    ],
    QuitConfirm: ["QUIT", "ЗАВЕРШИТЬ", "BEENDEN", "QUITTER", "SALIR", "SAIR"],
    Cancel: ["CANCEL", "ОТМЕНА", "ABBRECHEN", "ANNULER", "CANCELAR", "CANCELAR"],
    Quitting: ["Stopping the daemon…", "Завершаю демон…", "Daemon wird beendet…", "Arrêt du démon…", "Deteniendo el daemon…", "Encerrando o daemon…"],
    DaemonStoppedNotice: ["Daemon stopped.", "Демон остановлен.", "Daemon beendet.", "Démon arrêté.", "Daemon detenido.", "Daemon encerrado."],
    ToggleTimeout: [
        "The daemon did not answer. Check its state.",
        "Демон не ответил на команду. Проверь его состояние.",
        "Der Daemon hat nicht geantwortet. Prüfe seinen Zustand.",
        "Le démon n'a pas répondu. Vérifiez son état.",
        "El daemon no respondió. Comprueba su estado.",
        "O daemon não respondeu. Verifique o estado."
    ],
    LegacyRestart: [
        "Old daemon version: restarting it for the new settings…",
        "Демон старой версии: перезапускаю его для новых настроек…",
        "Alte Daemon-Version: Neustart für die neuen Einstellungen…",
        "Ancienne version du démon : redémarrage pour les nouveaux réglages…",
        "Versión antigua del daemon: reiniciando para los nuevos ajustes…",
        "Versão antiga do daemon: reiniciando para as novas configurações…"
    ],
    SaveFailed: [
        "Could not save the settings: {:#}",
        "Не удалось сохранить настройки: {:#}",
        "Einstellungen konnten nicht gespeichert werden: {:#}",
        "Impossible d'enregistrer les réglages : {:#}",
        "No se pudo guardar la configuración: {:#}",
        "Não foi possível salvar as configurações: {:#}"
    ],
    WorkerStartClicks: [
        "Could not start clicking: {:#}",
        "Не удалось запустить клики: {:#}",
        "Klicks konnten nicht gestartet werden: {:#}",
        "Impossible de démarrer les clics : {:#}",
        "No se pudieron iniciar los clics: {:#}",
        "Não foi possível iniciar os cliques: {:#}"
    ],
    WorkerStartDaemon: [
        "Could not start the daemon: {:#}",
        "Не удалось запустить демон: {:#}",
        "Daemon konnte nicht gestartet werden: {:#}",
        "Impossible de démarrer le démon : {:#}",
        "No se pudo iniciar el daemon: {:#}",
        "Não foi possível iniciar o daemon: {:#}"
    ],
    WorkerStopClicks: [
        "Could not stop clicking: {:#}",
        "Не удалось остановить клики: {:#}",
        "Klicks konnten nicht gestoppt werden: {:#}",
        "Impossible d'arrêter les clics : {:#}",
        "No se pudieron detener los clics: {:#}",
        "Não foi possível parar os cliques: {:#}"
    ],
    WorkerApply: [
        "Could not apply the settings: {:#}",
        "Не удалось применить настройки: {:#}",
        "Einstellungen konnten nicht übernommen werden: {:#}",
        "Impossible d'appliquer les réglages : {:#}",
        "No se pudo aplicar la configuración: {:#}",
        "Não foi possível aplicar as configurações: {:#}"
    ],
    WorkerHotkey: [
        "Could not apply the hotkey: {:#}",
        "Не удалось применить горячую клавишу: {:#}",
        "Tastenkürzel konnte nicht übernommen werden: {:#}",
        "Impossible d'appliquer le raccourci : {:#}",
        "No se pudo aplicar el atajo: {:#}",
        "Não foi possível aplicar o atalho: {:#}"
    ],
    WorkerHoldHotkey: [
        "Could not apply the hold key: {:#}",
        "Не удалось применить клавишу удержания: {:#}",
        "Haltetaste konnte nicht übernommen werden: {:#}",
        "Impossible d'appliquer la touche de maintien : {:#}",
        "No se pudo aplicar la tecla de mantener: {:#}",
        "Não foi possível aplicar a tecla de segurar: {:#}"
    ],
    WorkerPauseHotkey: [
        "Could not pause the hotkey: {:#}",
        "Не удалось приостановить горячую клавишу: {:#}",
        "Tastenkürzel konnte nicht pausiert werden: {:#}",
        "Impossible de mettre le raccourci en pause : {:#}",
        "No se pudo pausar el atajo: {:#}",
        "Não foi possível pausar o atalho: {:#}"
    ],
    WorkerResumeHotkey: [
        "Could not restore the hotkey: {:#}",
        "Не удалось восстановить горячую клавишу: {:#}",
        "Tastenkürzel konnte nicht wiederhergestellt werden: {:#}",
        "Impossible de restaurer le raccourci : {:#}",
        "No se pudo restaurar el atajo: {:#}",
        "Não foi possível restaurar o atalho: {:#}"
    ],
    WorkerStopOldDaemon: [
        "Could not stop the old daemon for the protocol update.",
        "Не удалось остановить старый демон для обновления протокола.",
        "Der alte Daemon konnte für das Protokoll-Update nicht beendet werden.",
        "Impossible d'arrêter l'ancien démon pour la mise à jour du protocole.",
        "No se pudo detener el daemon antiguo para actualizar el protocolo.",
        "Não foi possível parar o daemon antigo para atualizar o protocolo."
    ],
    WorkerRestartDaemon: [
        "Could not restart the daemon: {:#}",
        "Не удалось перезапустить демон: {:#}",
        "Daemon konnte nicht neu gestartet werden: {:#}",
        "Impossible de redémarrer le démon : {:#}",
        "No se pudo reiniciar el daemon: {:#}",
        "Não foi possível reiniciar o daemon: {:#}"
    ],
    WorkerResumeFailed: [
        "The daemon restarted, but clicking could not resume: {:#}",
        "Демон перезапущен, но клики не возобновлены: {:#}",
        "Daemon neu gestartet, aber Klicks konnten nicht fortgesetzt werden: {:#}",
        "Le démon a redémarré, mais les clics n'ont pas repris : {:#}",
        "El daemon se reinició, pero los clics no se reanudaron: {:#}",
        "O daemon reiniciou, mas os cliques não retomaram: {:#}"
    ],
    WorkerQuitFailed: [
        "Could not stop the daemon: {:#}",
        "Не удалось завершить демон: {:#}",
        "Daemon konnte nicht beendet werden: {:#}",
        "Impossible d'arrêter le démon : {:#}",
        "No se pudo detener el daemon: {:#}",
        "Não foi possível encerrar o daemon: {:#}"
    ],
    WorkerBadResponse: [
        "Invalid daemon response: {:#}",
        "Некорректный ответ демона: {:#}",
        "Ungültige Daemon-Antwort: {:#}",
        "Réponse invalide du démon : {:#}",
        "Respuesta no válida del daemon: {:#}",
        "Resposta inválida do daemon: {:#}"
    ],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_names_and_codes() {
        assert_eq!(Language::parse("Auto"), Some(Language::Auto));
        assert_eq!(Language::parse("ru"), Some(Language::Ru));
        assert_eq!(Language::parse("Deutsch"), Some(Language::De));
        assert_eq!(Language::parse("français"), Some(Language::Fr));
        assert_eq!(Language::parse("en-US"), None);
        assert_eq!(Language::parse("klingon"), None);
    }

    #[test]
    fn detects_languages_from_locales() {
        assert_eq!(lang_from_locale("ru_RU.UTF-8"), Some(Lang::Ru));
        assert_eq!(lang_from_locale("de-DE"), Some(Lang::De));
        assert_eq!(lang_from_locale("pt_BR"), Some(Lang::Pt));
        assert_eq!(lang_from_locale("es_ES@euro"), Some(Lang::Es));
        assert_eq!(lang_from_locale("fr_CA.UTF-8"), Some(Lang::Fr));
        assert_eq!(lang_from_locale("en_GB"), Some(Lang::En));
        assert_eq!(lang_from_locale("ja_JP"), None);
    }

    #[test]
    fn every_key_has_a_translation() {
        for language in Language::ALL {
            let lang = language.resolve();
            for key in [Key::TabClicks, Key::MultiHint, Key::AutostartOn] {
                assert!(!tr(lang, key).is_empty());
            }
        }
    }
}
