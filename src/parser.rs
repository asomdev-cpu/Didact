use crate::lexer::{Spanned, Token};
use crate::ast::*;
use std::collections::HashMap;

// ─────────────────────────────────────────────
//  STRUCTURE DU PARSER
// ─────────────────────────────────────────────

pub struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
    /// Ligne du dernier token consommé par advance()
    last_line: usize,
    /// Texte source, ligne par ligne (pour les propriétés lues telles quelles, comme `expr`)
    source_lines: Vec<String>,
}

/// Propriétés dont la valeur est le reste de la ligne, tel quel
/// (expressions et LaTeX : espaces, ^, \, ... conservés sans guillemets)
const RAW_LINE_PROPS: &[&str] = &["expr"];

impl Parser {
    pub fn new(tokens: Vec<Spanned>) -> Self {
        Parser { tokens, pos: 0, last_line: 1, source_lines: Vec::new() }
    }

    /// Bloc multilignes `key : |` : les lignes suivantes plus indentées que `key`,
    /// avec retours à la ligne et `|` conservés, indentation commune retirée.
    fn raw_block_value(&mut self, line: usize) -> Option<PropValue> {
        if *self.peek() != Token::Pipe || self.source_lines.is_empty() {
            return None;
        }
        let indent = |l: &str| l.len() - l.trim_start().len();
        let key_indent = indent(self.source_lines.get(line - 1)?);
        let mut block: Vec<&str> = Vec::new();
        let mut last = line;
        for (i, l) in self.source_lines.iter().enumerate().skip(line) {
            if !l.trim().is_empty() && indent(l) <= key_indent {
                break;
            }
            block.push(l);
            last = i + 1;
        }
        while block.last().is_some_and(|l| l.trim().is_empty()) {
            block.pop();
        }
        let common = block.iter().filter(|l| !l.trim().is_empty()).map(|l| indent(l)).min().unwrap_or(0);
        let text: String = block
            .iter()
            .map(|l| if l.len() >= common { &l[common..] } else { "" })
            .collect::<Vec<&str>>()
            .join("\n")
            .trim_end()
            .to_string();
        // On saute les tokens du bloc (le lexer a pu les découper n'importe comment)
        while self.line() <= last && *self.peek() != Token::EOF {
            self.advance();
        }
        Some(PropValue::Block(text))
    }

    /// Si `key` est une propriété « reste de la ligne » et que le source est connu :
    /// renvoie le texte après le `:` et saute les tokens de cette ligne.
    fn raw_line_value(&mut self, key: &str, line: usize) -> Option<PropValue> {
        if !RAW_LINE_PROPS.contains(&key) {
            return None;
        }
        let text = self.source_lines.get(line.checked_sub(1)?)?;
        let after = text.split_once(':')?.1;
        // Commentaire en fin de ligne : " # ..."
        let after = match after.find(" #") {
            Some(i) => &after[..i],
            None => after,
        };
        let mut value = after.trim().to_string();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = value[1..value.len() - 1].to_string();
        }
        while self.line() == line && *self.peek() != Token::EOF {
            self.advance();
        }
        Some(PropValue::Single(value))
    }

    /// Regarde le token courant sans avancer
    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    /// Ligne du token courant
    fn line(&self) -> usize {
        self.tokens[self.pos].line
    }

    /// Avance et retourne le token courant
    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].token.clone();
        self.last_line = self.tokens[self.pos].line;
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    /// Avance en exigeant un identifiant
    fn expect_ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            tok => Err(format!("ligne {} : attendu un identifiant, trouvé {:?}", self.last_line, tok)),
        }
    }

    /// Avance en exigeant un token précis
    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(())
        } else {
            Err(format!("ligne {} : attendu {:?}, trouvé {:?}", self.last_line, expected, tok))
        }
    }

    /// Vérifie si on est au début d'un nouvel objet ou d'une section.
    fn is_at_top_level(&self) -> bool {
        if matches!(self.peek(), Token::EOF | Token::LBracket) {
            return true;
        }
        if let Token::Ident(s) = self.peek() {
            if matches!(s.as_str(), "deffigure" | "defstyle" | "defgroup") {
                return true;
            }
        }
        false
    }
}

// ─────────────────────────────────────────────
//  POINT D'ENTRÉE
// ─────────────────────────────────────────────

pub fn parse(tokens: Vec<Spanned>) -> Result<Document, String> {
    let mut p = Parser::new(tokens);
    p.parse_document()
}

/// Comme `parse`, avec le texte source : permet de lire `expr` tel quel
/// (`expr : x^2 + 1` sans guillemets).
pub fn parse_with_source(tokens: Vec<Spanned>, source: &str) -> Result<Document, String> {
    let mut p = Parser::new(tokens);
    p.source_lines = source.lines().map(|l| l.to_string()).collect();
    p.parse_document()
}

impl Parser {
    fn parse_document(&mut self) -> Result<Document, String> {
        let mut config = Config::default();
        let mut styles = Vec::new();
        let mut figures = Vec::new();
        let mut timeline = Vec::new();

        while *self.peek() != Token::EOF {
            match self.peek() {
                Token::LBracket => {
                    self.advance(); // [
                    let section = self.expect_ident()?;
                    self.expect(&Token::RBracket)?;

                    match section.as_str() {
                        "config" => config = self.parse_config()?,
                        "figures" => {
                            let (s, f) = self.parse_figures_section()?;
                            styles = s;
                            figures = f;
                        }
                        "timeline" => timeline = self.parse_timeline_section()?,
                        unknown => {
                            eprintln!("Attention (ligne {}) : section inconnue '[{}]' ignorée", self.last_line, unknown);
                        }
                    }
                }
                _ => {
                    self.advance(); // skip tokens inattendus
                }
            }
        }

        Ok(Document { config, styles, figures, timeline })
    }

    // ─────────────────────────────────────────────
    //  SECTION [config]
    // ─────────────────────────────────────────────

    fn parse_config(&mut self) -> Result<Config, String> {
        let mut config = Config::default();

        loop {
            if matches!(self.peek(), Token::EOF | Token::LBracket) {
                break;
            }

            if let Token::Ident(key) = self.peek().clone() {
                config.lines.insert(key.clone(), self.line());
                self.advance();
                self.expect(&Token::Colon)?;

                match key.as_str() {
                    "window" => config.window = self.parse_window()?,
                    "background" => config.background = self.parse_ident_or_string()?,
                    "languages" => config.languages = self.parse_string_list()?,
                    "default_lang" => config.default_lang = self.expect_ident()?,
                    _ => {
                        eprintln!("Attention (ligne {}) : propriété config inconnue '{}' ignorée", self.last_line, key);
                        self.skip_value();
                    }
                }
            } else {
                self.advance();
            }
        }

        Ok(config)
    }

    fn parse_window(&mut self) -> Result<WindowConfig, String> {
        match self.peek().clone() {
            Token::Ident(s) if s == "auto" => {
                self.advance();
                Ok(WindowConfig::Auto)
            }
            Token::LBracket => {
                self.advance();
                let a = self.parse_number_or_ident()?;
                let sep = self.advance();
                let b = self.parse_number_or_ident()?;
                self.expect(&Token::RBracket)?;

                let a_u32 = a.parse::<u32>().unwrap_or(0);
                let b_u32 = b.parse::<u32>().unwrap_or(0);

                // [16:9] → Ratio, [1920,1080] → Size
                match sep {
                    Token::Colon => Ok(WindowConfig::Ratio(a_u32, b_u32)),
                    Token::Comma => Ok(WindowConfig::Size(a_u32, b_u32)),
                    _ => Err(format!("ligne {} : séparateur inattendu dans window : {:?}", self.last_line, sep)),
                }
            }
            tok => Err(format!("ligne {} : valeur window inattendue : {:?}", self.line(), tok)),
        }
    }

    // ─────────────────────────────────────────────
    //  SECTION [figures]
    // ─────────────────────────────────────────────

    fn parse_figures_section(&mut self) -> Result<(Vec<StyleDef>, Vec<FigureDef>), String> {
        let mut styles = Vec::new();
        let mut figures = Vec::new();
        // Ordre de définition, partagé entre styles, figures et groupes
        // (sert au validateur pour interdire les références en avant)
        let mut index = 0;

        loop {
            match self.peek().clone() {
                Token::EOF | Token::LBracket => break,
                Token::Ident(kw) => match kw.as_str() {
                    "defstyle" => {
                        self.advance();
                        styles.push(self.parse_style(index)?);
                        index += 1;
                    }
                    "deffigure" => {
                        self.advance();
                        figures.push(self.parse_figure(index)?);
                        index += 1;
                    }
                    "defgroup" => {
                        self.advance();
                        let mut fig = self.parse_figure(index)?;
                        index += 1;
                        fig.properties.insert(
                            "type".to_string(),
                            PropValue::Single("group".to_string()),
                        );
                        figures.push(fig);
                    }
                    _ => {
                        self.advance();
                    }
                },
                _ => {
                    self.advance();
                }
            }
        }

        Ok((styles, figures))
    }

    fn parse_style(&mut self, index: usize) -> Result<StyleDef, String> {
        let line = self.last_line; // ligne du mot-clé defstyle
        let name = self.expect_ident()?;
        self.expect(&Token::Equals)?;

        let mut extends = None;
        let mut properties = HashMap::new();
        let mut prop_lines = HashMap::new();

        loop {
            if self.is_at_top_level() {
                break;
            }

            if let Token::Ident(key) = self.peek().clone() {
                let line = self.line();
                prop_lines.insert(key.clone(), line);
                self.advance();
                self.expect(&Token::Colon)?;

                if key == "extends" {
                    extends = Some(self.expect_ident()?);
                } else if let Some(v) = self.raw_block_value(line) {
                    properties.insert(key, v);
                } else if let Some(v) = self.raw_line_value(&key, line) {
                    properties.insert(key, v);
                } else {
                    properties.insert(key, self.parse_prop_value()?);
                }
            } else {
                self.advance();
            }
        }

        Ok(StyleDef { name, index, line, extends, properties, prop_lines })
    }

    fn parse_figure(&mut self, index: usize) -> Result<FigureDef, String> {
        let line = self.last_line; // ligne du mot-clé deffigure / defgroup
        let name = self.expect_ident()?;
        self.expect(&Token::Equals)?;

        let mut properties = HashMap::new();
        let mut prop_lines = HashMap::new();

        loop {
            if self.is_at_top_level() {
                break;
            }

            if let Token::Ident(key) = self.peek().clone() {
                let line = self.line();
                prop_lines.insert(key.clone(), line);
                self.advance();
                self.expect(&Token::Colon)?;
                let value = match self.raw_block_value(line) {
                    Some(v) => v,
                    None => match self.raw_line_value(&key, line) {
                        Some(v) => v,
                        None => self.parse_prop_value()?,
                    },
                };
                properties.insert(key, value);
            } else {
                self.advance();
            }
        }

        Ok(FigureDef { name, index, line, properties, prop_lines })
    }

    fn parse_prop_value(&mut self) -> Result<PropValue, String> {
        match self.peek().clone() {
            // Bloc multilignes (|)
            Token::Pipe => {
                self.advance();
                // Pour l'instant on collecte les tokens jusqu'au prochain objet top-level
                let mut content = String::new();
                while !self.is_at_top_level() {
                    match self.advance() {
                        Token::Ident(s) => { content.push_str(&s); content.push(' '); }
                        Token::StringLit(s) => { content.push_str(&s); content.push(' '); }
                        Token::Number(n) => { content.push_str(&n.to_string()); content.push(' '); }
                        _ => {}
                    }
                }
                Ok(PropValue::Block(content.trim().to_string()))
            }

            // Tuple (x, y) ou (50%, 30%)
            Token::LParen => {
                self.advance();
                let mut items = Vec::new();
                loop {
                    match self.peek().clone() {
                        Token::RParen => { self.advance(); break; }
                        Token::Comma => { self.advance(); }
                        Token::Number(n) => { items.push(n.to_string()); self.advance(); }
                        Token::Percentage(s) => { items.push(s); self.advance(); }
                        Token::Ident(s) => { items.push(s); self.advance(); }
                        _ => { self.advance(); }
                    }
                }
                Ok(PropValue::Tuple(items))
            }

            // Liste [a, b, c] ou liste de points [(20%, 80%), (50%, 20%)]
            Token::LBracket => {
                self.advance();
                let mut items = Vec::new();
                loop {
                    match self.peek().clone() {
                        Token::RBracket | Token::EOF => { self.advance(); break; }
                        Token::Comma => { self.advance(); }
                        Token::Ident(s) => { items.push(s); self.advance(); }
                        Token::StringLit(s) => { items.push(s); self.advance(); }
                        Token::Number(n) => { items.push(n.to_string()); self.advance(); }
                        Token::Percentage(p) => { items.push(p); self.advance(); }
                        // Un point (x, y) est gardé en un seul élément "(x,y)"
                        Token::LParen => {
                            self.advance();
                            let mut point = String::from("(");
                            loop {
                                match self.peek().clone() {
                                    Token::RParen | Token::EOF => { self.advance(); break; }
                                    Token::Comma => { point.push(','); self.advance(); }
                                    Token::Number(n) => { point.push_str(&n.to_string()); self.advance(); }
                                    Token::Percentage(p) => { point.push_str(&p); self.advance(); }
                                    Token::Ident(s) => { point.push_str(&s); self.advance(); }
                                    _ => { self.advance(); }
                                }
                            }
                            point.push(')');
                            items.push(point);
                        }
                        _ => { self.advance(); }
                    }
                }
                Ok(PropValue::List(items))
            }

            // Pourcentage seul
            Token::Percentage(s) => {
                let s = s.clone();
                self.advance();
                Ok(PropValue::Percent(s))
            }

            // Nombre
            Token::Number(n) => {
                let n = n;
                self.advance();
                // Vérifier si suivi de px, s, cm, etc.
                if let Token::Ident(unit) = self.peek().clone() {
                    if matches!(unit.as_str(), "px" | "pt" | "s" | "ms" | "cm" | "mm" | "em") {
                        let val = format!("{}{}", n, unit);
                        self.advance();
                        return Ok(PropValue::Single(val));
                    }
                }
                Ok(PropValue::Number(n))
            }

            // Chaîne
            Token::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Ok(PropValue::Single(s))
            }

            // Identifiant (valeur simple ou expression)
            Token::Ident(s) => {
                let s = s.clone();
                self.advance();
                // Gérer les références dynamiques : center(fig), axes_xy(3, 4), angle(fig)
                if self.peek() == &Token::LParen {
                    let mut call = format!("{}(", s);
                    self.advance(); // (
                    loop {
                        match self.peek().clone() {
                            Token::RParen => { self.advance(); break; }
                            Token::Comma => { call.push(','); self.advance(); }
                            Token::Ident(p) => { call.push_str(&p); self.advance(); }
                            Token::Number(n) => { call.push_str(&n.to_string()); self.advance(); }
                            Token::Percentage(p) => { call.push_str(&p); self.advance(); }
                            _ => { self.advance(); }
                        }
                    }
                    call.push(')');
                    Ok(PropValue::Single(call))
                } else {
                    Ok(PropValue::Single(s))
                }
            }

            _ => {
                self.advance();
                Ok(PropValue::Single(String::new()))
            }
        }
    }

    // ─────────────────────────────────────────────
    //  SECTION [timeline]
    // ─────────────────────────────────────────────

    fn parse_timeline_section(&mut self) -> Result<Vec<TimelineEvent>, String> {
        let mut events = Vec::new();

        loop {
            match self.peek() {
                Token::EOF | Token::LBracket => break,
                Token::Number(_) => {
                    events.push(self.parse_timeline_event()?);
                }
                _ => {
                    self.advance();
                }
            }
        }

        Ok(events)
    }

    fn parse_timeline_event(&mut self) -> Result<TimelineEvent, String> {
        // timestamp
        let line = self.line();
        let timestamp = match self.advance() {
            Token::Number(n) => n,
            tok => return Err(format!("ligne {} : attendu un timestamp, trouvé {:?}", line, tok)),
        };

        // =>
        self.expect(&Token::Arrow)?;

        // {
        self.expect(&Token::LBrace)?;

        let mut actions = Vec::new();

        loop {
            match self.peek().clone() {
                Token::RBrace | Token::EOF => {
                    if *self.peek() == Token::RBrace {
                        self.advance();
                    }
                    break;
                }
                Token::Ident(key) => {
                    let action_line = self.line();
                    self.advance();
                    self.expect(&Token::Colon)?;

                    let action = match key.as_str() {
                        "start"   => TimelineAction::Start(self.parse_start_list()?),
                        "end"     => TimelineAction::End(self.parse_end_list()?),
                        "destroy" => TimelineAction::Destroy(self.parse_ident_array()?),
                        "anim"    => TimelineAction::Anim(self.parse_anim_list()?),
                        "comment" => TimelineAction::Comment(self.parse_string_list()?),
                        _ => {
                            eprintln!("Attention (ligne {}) : action timeline inconnue '{}' ignorée", action_line, key);
                            self.skip_bracket_content();
                            continue;
                        }
                    };
                    actions.push(LocatedAction { line: action_line, action });
                }
                _ => {
                    self.advance();
                }
            }
        }

        Ok(TimelineEvent { timestamp, line, actions })
    }

    /// Parse : [fig1, fig2 fadein(1s), audio musique.mp3 volume=0.5]
    fn parse_start_list(&mut self) -> Result<Vec<StartItem>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();

        loop {
            match self.peek().clone() {
                Token::RBracket | Token::EOF => { self.advance(); break; }
                Token::Comma => { self.advance(); }
                Token::Ident(s) => {
                    self.advance();
                    if s == "audio" {
                        // audio nom_fichier.mp3 [volume=x]
                        let file = self.parse_ident_or_string()?;
                        let mut volume = None;
                        if let Token::Ident(k) = self.peek().clone() {
                            if k == "volume" {
                                self.advance();
                                self.expect(&Token::Equals)?;
                                if let Token::Number(v) = self.advance() {
                                    volume = Some(v);
                                }
                            }
                        }
                        items.push(StartItem::Audio { file, volume });
                    } else {
                        // Optionnellement une animation d'entrée : fig fadein(1s)
                        let anim = self.parse_optional_anim()?;
                        items.push(StartItem::Figure { name: s, anim });
                    }
                }
                _ => { self.advance(); }
            }
        }

        Ok(items)
    }

    /// Parse : [fig1, fig2 fadeout(0.5s), audio musique.mp3]
    fn parse_end_list(&mut self) -> Result<Vec<EndItem>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();

        loop {
            match self.peek().clone() {
                Token::RBracket | Token::EOF => { self.advance(); break; }
                Token::Comma => { self.advance(); }
                Token::Ident(name) => {
                    self.advance();
                    // end : [audio musique.mp3]
                    if name == "audio" {
                        let file = self.parse_ident_or_string()?;
                        items.push(EndItem { name: file, anim: None, audio: true });
                        continue;
                    }
                    // Optionnellement une animation de sortie
                    let anim = self.parse_optional_anim()?;
                    items.push(EndItem { name, anim, audio: false });
                }
                _ => { self.advance(); }
            }
        }

        Ok(items)
    }

    /// Une animation optionnelle juste après un nom de figure (`fig fadein(1s)`)
    fn parse_optional_anim(&mut self) -> Result<Option<AnimCall>, String> {
        if let Token::Ident(_) = self.peek() {
            Ok(Some(self.parse_anim_call()?))
        } else {
            Ok(None)
        }
    }

    /// Parse : [fig1 move(to=(80%, 50%), 2s), fig2 [fadein(0.5s), color(to=red, 1s)]]
    /// Une liste entre crochets après la figure = animations enchaînées (l'une après l'autre).
    fn parse_anim_list(&mut self) -> Result<Vec<AnimItem>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();

        loop {
            match self.peek().clone() {
                Token::RBracket | Token::EOF => { self.advance(); break; }
                Token::Comma => { self.advance(); }
                Token::Ident(figure) => {
                    self.advance();
                    let mut calls = Vec::new();
                    if *self.peek() == Token::LBracket {
                        self.advance();
                        loop {
                            match self.peek().clone() {
                                Token::RBracket | Token::EOF => { self.advance(); break; }
                                Token::Comma => { self.advance(); }
                                Token::Ident(_) => calls.push(self.parse_anim_call()?),
                                _ => { self.advance(); }
                            }
                        }
                    } else {
                        calls.push(self.parse_anim_call()?);
                    }
                    items.push(AnimItem { figure, calls });
                }
                _ => { self.advance(); }
            }
        }

        Ok(items)
    }

    /// Parse un appel d'animation : move(to=(50%, 50%), 2s, ease=linear)
    /// Les paramètres sont gardés sous forme de texte : "to=(50%,50%)", "2s", "ease=linear".
    fn parse_anim_call(&mut self) -> Result<AnimCall, String> {
        let line = self.line();
        let name = self.expect_ident()?;
        let mut params = Vec::new();

        if *self.peek() == Token::LParen {
            self.advance();
            loop {
                match self.peek().clone() {
                    Token::RParen | Token::EOF => { self.advance(); break; }
                    Token::Comma => { self.advance(); }
                    Token::Ident(key) => {
                        self.advance();
                        if *self.peek() == Token::Equals {
                            // Paramètre nommé : to=..., path=..., ease=...
                            self.advance();
                            let val = self.parse_param_value();
                            params.push(format!("{}={}", key, val));
                        } else if *self.peek() == Token::LParen {
                            // Valeur appel sans nom : center(b)
                            params.push(format!("{}{}", key, self.parse_paren_group()));
                        } else {
                            params.push(key);
                        }
                    }
                    _ => params.push(self.parse_param_value()),
                }
            }
        }

        Ok(AnimCall { name, params, line })
    }

    /// Une valeur de paramètre : 2s, 50%, red, #FF0000, (50%, 50%), center(b), -45
    fn parse_param_value(&mut self) -> String {
        match self.peek().clone() {
            Token::LParen => self.parse_paren_group(),
            Token::Number(n) => {
                self.advance();
                let mut val = n.to_string();
                // unité collée : s, ms, px, deg, ...
                if let Token::Ident(unit) = self.peek().clone() {
                    if matches!(unit.as_str(), "s" | "ms" | "px" | "cm" | "deg" | "deg/s" | "cm/s" | "px/s") {
                        val.push_str(&unit);
                        self.advance();
                    }
                }
                val
            }
            Token::Ident(s) => {
                self.advance();
                if *self.peek() == Token::LParen {
                    format!("{}{}", s, self.parse_paren_group())
                } else {
                    s
                }
            }
            Token::Percentage(p) => { self.advance(); p }
            Token::StringLit(s) => { self.advance(); s }
            _ => { self.advance(); String::new() }
        }
    }

    /// "(a, b)" → "(a,b)" ; gère les parenthèses imbriquées
    fn parse_paren_group(&mut self) -> String {
        let mut out = String::new();
        let mut depth = 0;
        loop {
            match self.peek().clone() {
                Token::LParen => { depth += 1; out.push('('); self.advance(); }
                Token::RParen => {
                    depth -= 1;
                    out.push(')');
                    self.advance();
                    if depth <= 0 { break; }
                }
                Token::EOF => break,
                Token::Comma => { out.push(','); self.advance(); }
                Token::Number(n) => { out.push_str(&n.to_string()); self.advance(); }
                Token::Percentage(p) => { out.push_str(&p); self.advance(); }
                Token::Ident(s) => { out.push_str(&s); self.advance(); }
                Token::StringLit(s) => { out.push_str(&s); self.advance(); }
                _ => { self.advance(); }
            }
        }
        out
    }

    // ─────────────────────────────────────────────
    //  UTILITAIRES
    // ─────────────────────────────────────────────

    fn parse_ident_or_string(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            Token::StringLit(s) => Ok(s),
            tok => Err(format!("ligne {} : attendu ident ou string, trouvé {:?}", self.last_line, tok)),
        }
    }

    fn parse_number_or_ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Number(n) => Ok(n.to_string()),
            Token::Ident(s) => Ok(s),
            tok => Err(format!("ligne {} : attendu nombre ou ident, trouvé {:?}", self.last_line, tok)),
        }
    }

    fn parse_string_list(&mut self) -> Result<Vec<String>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();
        loop {
            match self.peek().clone() {
                Token::RBracket => { self.advance(); break; }
                Token::Comma => { self.advance(); }
                Token::StringLit(s) => { items.push(s); self.advance(); }
                Token::Ident(s) => { items.push(s); self.advance(); }
                _ => { self.advance(); }
            }
        }
        Ok(items)
    }

    fn parse_ident_array(&mut self) -> Result<Vec<String>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();
        loop {
            match self.peek().clone() {
                Token::RBracket => { self.advance(); break; }
                Token::Comma => { self.advance(); }
                Token::Ident(s) => { items.push(s); self.advance(); }
                _ => { self.advance(); }
            }
        }
        Ok(items)
    }

    /// Ignore une valeur simple (pour les props inconnues)
    fn skip_value(&mut self) {
        match self.peek().clone() {
            Token::LBracket | Token::LParen => {
                self.skip_bracket_content();
            }
            _ => { self.advance(); }
        }
    }

    /// Ignore un contenu entre crochets/parenthèses
    fn skip_bracket_content(&mut self) {
        let open = self.advance();
        let close = match open {
            Token::LBracket => Token::RBracket,
            Token::LParen => Token::RParen,
            Token::LBrace => Token::RBrace,
            _ => return,
        };
        let mut depth = 1;
        while depth > 0 && *self.peek() != Token::EOF {
            let tok = self.advance();
            if std::mem::discriminant(&tok) == std::mem::discriminant(&open) {
                depth += 1;
            } else if std::mem::discriminant(&tok) == std::mem::discriminant(&close) {
                depth -= 1;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn expr_lue_telle_quelle() {
        let src = "[figures]
deffigure p =
    type : plot
    expr : x^2 + 2x - 1   # parabole
    axes : a";
        let doc = parse_with_source(tokenize(src), src).unwrap();
        let f = &doc.figures[0];
        assert_eq!(f.properties["expr"], PropValue::Single("x^2 + 2x - 1".into()));
        assert_eq!(f.properties["axes"], PropValue::Single("a".into())); // la ligne suivante est intacte
    }

    #[test]
    fn bloc_multiligne() {
        let src = "[figures]
deffigure t =
    type   : table
    rows   : |
        0 | 1  | 0
        1 | e  | e

    header : [x, f(x)]
deffigure u =
    type : text";
        let doc = parse_with_source(tokenize(src), src).unwrap();
        let f = &doc.figures[0];
        assert_eq!(f.properties["rows"], PropValue::Block("0 | 1  | 0\n1 | e  | e".into()));
        assert!(f.properties.contains_key("header"));
        assert_eq!(doc.figures.len(), 2);
    }

    #[test]
    fn liste_de_points() {
        let doc = parse(tokenize(
            "[figures]
            deffigure t =
                type   : polygon
                points : [(20%, 80%), (50%, 20%), (80%, 80%)]",
        ))
        .unwrap();
        match &doc.figures[0].properties["points"] {
            PropValue::List(items) => assert_eq!(items, &["(20%,80%)", "(50%,20%)", "(80%,80%)"]),
            other => panic!("attendu une liste, trouvé {:?}", other),
        }
    }
}
