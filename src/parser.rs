use crate::lexer::Token;
use crate::ast::*;
use std::collections::HashMap;

// ─────────────────────────────────────────────
//  STRUCTURE DU PARSER
// ─────────────────────────────────────────────

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    /// Regarde le token courant sans avancer
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    /// Regarde le token suivant sans avancer
    fn peek_next(&self) -> &Token {
        if self.pos + 1 < self.tokens.len() {
            &self.tokens[self.pos + 1]
        } else {
            &Token::EOF
        }
    }

    /// Avance et retourne le token courant
    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    /// Avance en exigeant un identifiant
    fn expect_ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            tok => Err(format!("Attendu un identifiant, trouvé {:?} (pos {})", tok, self.pos)),
        }
    }

    /// Avance en exigeant un token précis
    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(())
        } else {
            Err(format!("Attendu {:?}, trouvé {:?} (pos {})", expected, tok, self.pos))
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

pub fn parse(tokens: Vec<Token>) -> Result<Document, String> {
    let mut p = Parser::new(tokens);
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
                            eprintln!("Attention : section inconnue '[{}]' ignorée", unknown);
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
                self.advance();
                self.expect(&Token::Colon)?;

                match key.as_str() {
                    "window" => config.window = self.parse_window()?,
                    "background" => config.background = self.parse_ident_or_string()?,
                    "languages" => config.languages = self.parse_string_list()?,
                    "default_lang" => config.default_lang = self.expect_ident()?,
                    _ => {
                        eprintln!("Attention : propriété config inconnue '{}' ignorée", key);
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
                    _ => Err(format!("Séparateur inattendu dans window : {:?}", sep)),
                }
            }
            tok => Err(format!("Valeur window inattendue : {:?}", tok)),
        }
    }

    // ─────────────────────────────────────────────
    //  SECTION [figures]
    // ─────────────────────────────────────────────

    fn parse_figures_section(&mut self) -> Result<(Vec<StyleDef>, Vec<FigureDef>), String> {
        let mut styles = Vec::new();
        let mut figures = Vec::new();

        loop {
            match self.peek().clone() {
                Token::EOF | Token::LBracket => break,
                Token::Ident(kw) => match kw.as_str() {
                    "defstyle" => {
                        self.advance();
                        styles.push(self.parse_style()?);
                    }
                    "deffigure" => {
                        self.advance();
                        figures.push(self.parse_figure()?);
                    }
                    "defgroup" => {
                        self.advance();
                        let mut fig = self.parse_figure()?;
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

    fn parse_style(&mut self) -> Result<StyleDef, String> {
        let name = self.expect_ident()?;
        self.expect(&Token::Equals)?;

        let mut extends = None;
        let mut properties = HashMap::new();

        loop {
            if self.is_at_top_level() {
                break;
            }

            if let Token::Ident(key) = self.peek().clone() {
                self.advance();
                self.expect(&Token::Colon)?;

                if key == "extends" {
                    extends = Some(self.expect_ident()?);
                } else {
                    properties.insert(key, self.parse_prop_value()?);
                }
            } else {
                self.advance();
            }
        }

        Ok(StyleDef { name, extends, properties })
    }

    fn parse_figure(&mut self) -> Result<FigureDef, String> {
        let name = self.expect_ident()?;
        self.expect(&Token::Equals)?;

        let mut properties = HashMap::new();

        loop {
            if self.is_at_top_level() {
                break;
            }

            if let Token::Ident(key) = self.peek().clone() {
                self.advance();
                self.expect(&Token::Colon)?;
                properties.insert(key, self.parse_prop_value()?);
            } else {
                self.advance();
            }
        }

        Ok(FigureDef { name, properties })
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

            // Liste [a, b, c]
            Token::LBracket => {
                self.advance();
                let mut items = Vec::new();
                loop {
                    match self.peek().clone() {
                        Token::RBracket => { self.advance(); break; }
                        Token::Comma => { self.advance(); }
                        Token::Ident(s) => { items.push(s); self.advance(); }
                        Token::StringLit(s) => { items.push(s); self.advance(); }
                        Token::Number(n) => { items.push(n.to_string()); self.advance(); }
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
        let timestamp = match self.advance() {
            Token::Number(n) => n,
            tok => return Err(format!("Attendu un timestamp, trouvé {:?}", tok)),
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
                    self.advance();
                    self.expect(&Token::Colon)?;

                    match key.as_str() {
                        "start"   => actions.push(TimelineAction::Start(self.parse_start_list()?)),
                        "end"     => actions.push(TimelineAction::End(self.parse_end_list()?)),
                        "destroy" => actions.push(TimelineAction::Destroy(self.parse_ident_array()?)),
                        "anim"    => actions.push(TimelineAction::Anim(self.parse_anim_list()?)),
                        "comment" => actions.push(TimelineAction::Comment(self.parse_string_list()?)),
                        _ => {
                            eprintln!("Attention : action timeline inconnue '{}' ignorée", key);
                            self.skip_bracket_content();
                        }
                    }
                }
                _ => {
                    self.advance();
                }
            }
        }

        Ok(TimelineEvent { timestamp, actions })
    }

    /// Parse : [fig1, fig2, audio musique.mp3 volume=0.5]
    fn parse_start_list(&mut self) -> Result<Vec<StartItem>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();

        loop {
            match self.peek().clone() {
                Token::RBracket => { self.advance(); break; }
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
                        items.push(StartItem::Figure(s));
                    }
                }
                _ => { self.advance(); }
            }
        }

        Ok(items)
    }

    /// Parse : [fig1, fig2 fadeout(0.5s)]
    fn parse_end_list(&mut self) -> Result<Vec<EndItem>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();

        loop {
            match self.peek().clone() {
                Token::RBracket => { self.advance(); break; }
                Token::Comma => { self.advance(); }
                Token::Ident(name) => {
                    self.advance();
                    // Optionnellement une animation de sortie
                    let anim = if let Token::Ident(_) = self.peek() {
                        Some(self.parse_anim_call_string()?)
                    } else {
                        None
                    };
                    items.push(EndItem { name, anim });
                }
                _ => { self.advance(); }
            }
        }

        Ok(items)
    }

    /// Parse : [fig1 move(down, 2s), fig2 fadein(0.5s)]
    fn parse_anim_list(&mut self) -> Result<Vec<AnimItem>, String> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();

        loop {
            match self.peek().clone() {
                Token::RBracket => { self.advance(); break; }
                Token::Comma => { self.advance(); }
                Token::Ident(figure) => {
                    self.advance();
                    // Une ou plusieurs animations enchaînées
                    if *self.peek() == Token::LBracket {
                        // animations enchaînées : [move(...), color(...)]
                        self.advance();
                        loop {
                            match self.peek().clone() {
                                Token::RBracket => { self.advance(); break; }
                                Token::Comma => { self.advance(); }
                                Token::Ident(_) => {
                                    let call = self.parse_anim_call()?;
                                    items.push(AnimItem { figure: figure.clone(), call });
                                }
                                _ => { self.advance(); }
                            }
                        }
                    } else {
                        let call = self.parse_anim_call()?;
                        items.push(AnimItem { figure, call });
                    }
                }
                _ => { self.advance(); }
            }
        }

        Ok(items)
    }

    /// Parse un appel d'animation : move(down, 2s)
    fn parse_anim_call(&mut self) -> Result<AnimCall, String> {
        let name = self.expect_ident()?;
        let mut params = Vec::new();

        if *self.peek() == Token::LParen {
            self.advance();
            loop {
                match self.peek().clone() {
                    Token::RParen => { self.advance(); break; }
                    Token::Comma => { self.advance(); }
                    Token::Ident(s) => {
                        // Gérer les paramètres nommés : to=(50%, 50%)
                        let key = s.clone();
                        self.advance();
                        if *self.peek() == Token::Equals {
                            self.advance();
                            let val = match self.peek().clone() {
                                Token::LParen => {
                                    self.advance();
                                    let mut tuple = String::from("(");
                                    loop {
                                        match self.peek().clone() {
                                            Token::RParen => { self.advance(); break; }
                                            Token::Comma => { tuple.push(','); self.advance(); }
                                            Token::Percentage(p) => { tuple.push_str(&p); self.advance(); }
                                            Token::Number(n) => { tuple.push_str(&n.to_string()); self.advance(); }
                                            Token::Ident(s) => { tuple.push_str(&s); self.advance(); }
                                            _ => { self.advance(); }
                                        }
                                    }
                                    tuple.push(')');
                                    tuple
                                }
                                Token::Number(n) => { let v = n.to_string(); self.advance(); v }
                                Token::Ident(s) => { let v = s.clone(); self.advance(); v }
                                Token::Percentage(s) => { let v = s.clone(); self.advance(); v }
                                _ => { self.advance(); String::new() }
                            };
                            params.push(format!("{}={}", key, val));
                        } else {
                            params.push(key);
                        }
                    }
                    Token::Number(n) => {
                        let mut val = n.to_string();
                        self.advance();
                        // unité : s, ms, px, cm, deg
                        if let Token::Ident(unit) = self.peek().clone() {
                            if matches!(unit.as_str(), "s" | "ms" | "px" | "cm" | "deg" | "deg/s") {
                                val.push_str(&unit);
                                self.advance();
                            }
                        }
                        params.push(val);
                    }
                    Token::Percentage(p) => { params.push(p); self.advance(); }
                    Token::StringLit(s) => { params.push(s); self.advance(); }
                    _ => { self.advance(); }
                }
            }
        }

        Ok(AnimCall { name, params })
    }

    /// Version string de parse_anim_call (pour end avec animation)
    fn parse_anim_call_string(&mut self) -> Result<String, String> {
        let call = self.parse_anim_call()?;
        let params = call.params.join(", ");
        Ok(format!("{}({})", call.name, params))
    }

    // ─────────────────────────────────────────────
    //  UTILITAIRES
    // ─────────────────────────────────────────────

    fn parse_ident_or_string(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            Token::StringLit(s) => Ok(s),
            tok => Err(format!("Attendu ident ou string, trouvé {:?}", tok)),
        }
    }

    fn parse_number_or_ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Token::Number(n) => Ok(n.to_string()),
            Token::Ident(s) => Ok(s),
            tok => Err(format!("Attendu nombre ou ident, trouvé {:?}", tok)),
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