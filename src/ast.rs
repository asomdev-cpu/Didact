use std::collections::HashMap;

/// Un document Didact complet (fichier .dct)
#[derive(Debug, Clone)]
pub struct Document {
    pub config: Config,
    pub styles: Vec<StyleDef>,
    pub figures: Vec<FigureDef>,
    pub timeline: Vec<TimelineEvent>,
}

// ─────────────────────────────────────────────
//  CONFIG
// ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Config {
    pub window: WindowConfig,
    pub background: String,
    pub languages: Vec<String>,
    pub default_lang: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window: WindowConfig::Auto,
            background: "white".to_string(),
            languages: vec!["FR".to_string()],
            default_lang: "FR".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum WindowConfig {
    Auto,
    Size(u32, u32),   // [1920, 1080]
    Ratio(u32, u32),  // [16:9]
}

// ─────────────────────────────────────────────
//  FIGURES & STYLES
// ─────────────────────────────────────────────

/// Un style réutilisable
#[derive(Debug, Clone)]
pub struct StyleDef {
    pub name: String,
    pub extends: Option<String>,
    pub properties: HashMap<String, PropValue>,
}

/// Une figure (objet visuel)
#[derive(Debug, Clone)]
pub struct FigureDef {
    pub name: String,
    pub properties: HashMap<String, PropValue>,
}

/// Une valeur de propriété
#[derive(Debug, Clone)]
pub enum PropValue {
    /// Valeur simple : "red", "auto", "24px"
    Single(String),

    /// Pourcentage : "50%"
    Percent(String),

    /// Nombre : 3.14
    Number(f64),

    /// Tuple de position : (50%, 50%) ou (10, 20)
    Tuple(Vec<String>),

    /// Liste : [FR, NL, EN] ou [fig1, fig2]
    List(Vec<String>),

    /// Bloc multilignes (après |)
    Block(String),
}

// ─────────────────────────────────────────────
//  TIMELINE
// ─────────────────────────────────────────────

/// Un événement dans la timeline
/// Ex : 5.000 => { start : [ma_ligne] }
#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub timestamp: f64,   // en secondes.millisecondes
    pub actions: Vec<TimelineAction>,
}

/// Une action dans un bloc timeline
#[derive(Debug, Clone)]
pub enum TimelineAction {
    /// start : [fig1, fig2]
    Start(Vec<StartItem>),

    /// end : [fig1, fig2]
    End(Vec<EndItem>),

    /// destroy : [fig1, fig2]
    Destroy(Vec<String>),

    /// anim : [fig1 move(down, 2s), fig2 fadein(0.5s)]
    Anim(Vec<AnimItem>),

    /// comment : ["bonjour", "hallo", "hello"]
    Comment(Vec<String>),
}

/// Un élément dans start (peut inclure audio)
#[derive(Debug, Clone)]
pub enum StartItem {
    Figure(String),
    Audio { file: String, volume: Option<f64> },
}

/// Un élément dans end (peut inclure une animation de sortie)
#[derive(Debug, Clone)]
pub struct EndItem {
    pub name: String,
    pub anim: Option<String>, // ex: fadeout(0.5s)
}

/// Une animation appliquée à une figure
#[derive(Debug, Clone)]
pub struct AnimItem {
    pub figure: String,
    pub call: AnimCall,
}

/// Un appel d'animation : move(down, 2s)
#[derive(Debug, Clone)]
pub struct AnimCall {
    pub name: String,
    pub params: Vec<String>,
}