use crate::lexer::{Spanned, Token};
use crate::ast::*;
use crate::model::{self, Model};
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
    /// Modèles déjà définis (`defmodel`)
    models: HashMap<String, Model>,
    /// Paramètres déjà définis (`defparam`) : pour lire `radius : 2*k`
    param_names: Vec<String>,
    /// Fonctions définies (deffunction)
    functions: Vec<String>,
}

/// Propriétés dont la valeur est le reste de la ligne, tel quel
/// (expressions et LaTeX : espaces, ^, \, ... conservés sans guillemets)
const RAW_LINE_PROPS: &[&str] = &["expr", "polar", "part", "cmap", "upper", "lower", "where", "domain", "escape", "z0", "edges", "values", "weights", "sample", "x", "y"];

impl Parser {
    pub fn new(tokens: Vec<Spanned>) -> Self {
        Parser { tokens, pos: 0, last_line: 1, source_lines: Vec::new(), models: HashMap::new(), param_names: Vec::new(), functions: Vec::new() }
    }

    /// Bloc multilignes `key : |` : les lignes suivantes plus indentées que `key`,
    /// avec retours à la ligne et `|` conservés, indentation commune retirée.
    fn raw_block_value(&mut self, line: usize) -> Option<PropValue> {
        if *self.peek() != Token::Pipe || self.source_lines.is_empty() {
            return None;
        }
        // Un bloc seulement si `|` est seul après les deux-points : `expr : |x| + 1` est une
        // formule qui commence par une valeur absolue, pas un bloc
        let after = self.source_lines.get(line - 1)?.split_once(':')?.1;
        let after = after.split(" #").next().unwrap_or(after).trim();
        if after != "|" {
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
        // Commentaire en fin de ligne : " # ..." (mais pas un nombre dynamique " #{n}")
        let after = match after.match_indices(" #").find(|(i, _)| !after[i + 2..].starts_with('{')) {
            Some((i, _)) => &after[..i],
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

    /// Valeur de la forme `nom(arguments)` qui occupe toute la fin de la ligne :
    /// gardée telle quelle (sans espaces), pour que les coordonnées restent des expressions
    /// (`repere(a - 1, sin(a))`). Le découpage en tokens perdrait les opérateurs.
    fn raw_call_value(&mut self, line: usize) -> Option<PropValue> {
        let Token::Ident(name) = self.peek().clone() else { return None };
        if self.tokens.get(self.pos + 1).map(|t| &t.token) != Some(&Token::LParen) {
            return None;
        }
        let text = self.source_lines.get(line.checked_sub(1)?)?;
        let after = text.split_once(':')?.1;
        let after = match after.find(" #") {
            Some(i) => &after[..i],
            None => after,
        };
        let value: String = after.chars().filter(|c| !c.is_whitespace()).collect();
        let args = value.strip_prefix(&name)?.strip_prefix('(')?.strip_suffix(')')?;
        // Parenthèses équilibrées : la dernière ferme bien la première
        let mut depth = 0i32;
        for c in args.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return None;
            }
        }
        if depth != 0 {
            return None;
        }
        while self.line() == line && *self.peek() != Token::EOF {
            self.advance();
        }
        Some(PropValue::Single(value))
    }

    /// Expression de paramètres qui occupe toute la fin de la ligne, avec son unité éventuelle :
    /// `2*k`, `(k + 1)px`, `k/4`, `(2k)%`. Gardée telle quelle (sans espaces), calculée à chaque image.
    fn raw_expr_value(&mut self, line: usize) -> Option<PropValue> {
        if self.param_names.is_empty() || matches!(self.peek(), Token::StringLit(_) | Token::LBracket | Token::Pipe) {
            return None;
        }
        let text = self.source_lines.get(line.checked_sub(1)?)?;
        let after = text.split_once(':')?.1;
        let after = match after.find(" # ") {
            Some(i) => &after[..i],
            None => after,
        };
        let value: String = after.chars().filter(|c| !c.is_whitespace()).collect();
        crate::renderer::params::param_expr(&value, &self.param_names)?;
        while self.line() == line && *self.peek() != Token::EOF {
            self.advance();
        }
        Some(PropValue::Single(value))
    }

    /// Liste `[ ... ]` sur une ligne dont un élément contient une expression de paramètres :
    /// `[(10%, 10%), (k*15%, 30%)]`. Chaque élément est gardé tel quel (sans espaces).
    fn raw_list_value(&mut self, line: usize) -> Option<PropValue> {
        if self.param_names.is_empty() || *self.peek() != Token::LBracket {
            return None;
        }
        let text = self.source_lines.get(line.checked_sub(1)?)?;
        let after = text.split_once(':')?.1;
        let after = match after.find(" # ") {
            Some(i) => &after[..i],
            None => after,
        };
        let value: String = after.chars().filter(|c| !c.is_whitespace()).collect();
        let inside = value.strip_prefix('[')?.strip_suffix(']')?;
        let call = format!("l({})", inside);
        let (_, items) = crate::renderer::geometry::split_call(&call);
        let items: Vec<String> = items.iter().map(|s| s.to_string()).collect();
        let is_expr = |s: &str| crate::renderer::params::param_expr(s, &self.param_names).is_some();
        let dynamic = items.iter().any(|item| {
            if let Some(point) = item.strip_prefix('(').and_then(|p| p.strip_suffix(')')) {
                let call = format!("p({})", point);
                let (_, coords) = crate::renderer::geometry::split_call(&call);
                coords.iter().any(|c| is_expr(c))
            } else {
                is_expr(item)
            }
        });
        if !dynamic {
            return None;
        }
        while self.line() == line && *self.peek() != Token::EOF {
            self.advance();
        }
        Some(PropValue::List(items))
    }

    /// Après une valeur : il ne doit rien rester sur la ligne (sinon la valeur a été mal comprise,
    /// typiquement un calcul avec un paramètre non défini : `radius : 2*q`)
    fn check_line_consumed(&self, key: &str, line: usize) -> Result<(), String> {
        if self.line() != line || matches!(self.peek(), Token::EOF) {
            return Ok(());
        }
        // Une autre propriété sur la même ligne : permis
        if matches!(self.peek(), Token::Ident(_)) && self.tokens.get(self.pos + 1).map(|t| &t.token) == Some(&Token::Colon) {
            return Ok(());
        }
        let text = self.source_lines.get(line - 1).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
        let hint = if self.param_names.is_empty() {
            "un calcul n'est permis qu'avec des paramètres (defparam) définis plus haut".to_string()
        } else {
            format!("calcul illisible ou nom inconnu (paramètres définis plus haut : {})", self.param_names.join(", "))
        };
        Err(format!("ligne {} : `{}` : valeur '{}' non comprise : {}", line, key, text, hint))
    }

    /// Valeur `( ... )` qui occupe toute la fin de la ligne et contient des calculs
    /// (`(0.9*vx - 0.05*dx, sin(x))`, `(60, theta + 10)`) : chaque élément est gardé tel quel.
    /// Les tuples simples (`(50%, 30%)`, `(1, 2)`) gardent la lecture habituelle.
    fn raw_tuple_value(&mut self, line: usize) -> Option<PropValue> {
        if *self.peek() != Token::LParen {
            return None;
        }
        let text = self.source_lines.get(line.checked_sub(1)?)?;
        let after = text.split_once(':')?.1;
        // Commentaire de fin de ligne : « # » suivi d'une espace (une couleur #FF0000 n'en a pas)
        let after = match after.find(" # ") {
            Some(i) => &after[..i],
            None => after,
        };
        let value = after.trim();
        let inside = value.strip_prefix('(')?.strip_suffix(')')?;
        let mut items = Vec::new();
        let (mut depth, mut start) = (0i32, 0);
        for (i, c) in inside.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth < 0 {
                        return None; // « (a) + (b) » : pas un seul tuple
                    }
                }
                ',' if depth == 0 => {
                    items.push(inside[start..i].trim().to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
        if depth != 0 {
            return None;
        }
        items.push(inside[start..].trim().to_string());
        let simple = |s: &str| {
            let n = s.trim_end_matches('%').trim_end_matches("px");
            n.parse::<f64>().is_ok() || s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '#')
        };
        if items.iter().all(|i| simple(i)) {
            return None;
        }
        while self.line() == line && *self.peek() != Token::EOF {
            self.advance();
        }
        Some(PropValue::Tuple(items))
    }

    /// Regarde le token courant sans avancer
    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    /// Regarde le token `k` places plus loin (EOF au-delà)
    fn peek_at(&self, k: usize) -> &Token {
        let i = (self.pos + k).min(self.tokens.len() - 1);
        &self.tokens[i].token
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

    /// Fin du fichier atteinte sans le crochet ou la parenthèse fermante
    fn unclosed(&self, close: &str) -> String {
        format!("ligne {} : `{}` manquant : fin du fichier atteinte avant la fermeture", self.last_line, close)
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
            if matches!(s.as_str(), "deffigure" | "defstyle" | "defgroup" | "defparam" | "defmodel" | "defrepeat" | "deffunction") {
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
    // Nouveau document : les fonctions d'un fichier précédent sont oubliées (pas dans les tests,
    // qui lisent plusieurs documents en parallèle)
    #[cfg(not(test))]
    crate::renderer::expr::clear_functions();
    let doc = p.parse_document()?;
    #[cfg(not(test))]
    crate::renderer::params::DECIMAL_COMMA.store(doc.config.decimal_comma, std::sync::atomic::Ordering::Relaxed);
    Ok(doc)
}

impl Parser {
    fn parse_document(&mut self) -> Result<Document, String> {
        if let Some(t) = self.tokens.iter().find(|t| t.token == Token::Unterminated) {
            // Le guillemet oublié est souvent plus haut : première ligne qui en compte un nombre impair
            let odd = self.source_lines.iter().position(|l| l.matches('"').count() % 2 == 1).map(|i| i + 1);
            let hint = match odd {
                Some(l) if l != t.line => format!(" (premier guillemet sans son double : ligne {})", l),
                _ => String::new(),
            };
            return Err(format!("ligne {} : guillemet `\"` fermant manquant{}", t.line, hint));
        }
        let mut config = Config::default();
        let mut styles = Vec::new();
        let mut params = Vec::new();
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
                            let (s, p, f) = self.parse_figures_section()?;
                            styles = s;
                            params = p;
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

        Ok(Document { config, styles, params, figures, timeline })
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
                    "decimal" => {
                        config.decimal_comma = match self.expect_ident()?.as_str() {
                            "comma" => true,
                            "point" => false,
                            other => return Err(format!("ligne {} : decimal : '{}' invalide (comma ou point)", self.last_line, other)),
                        }
                    }
                    _ => {
                        const KNOWN: &[&str] = &["window", "background", "languages", "default_lang", "decimal"];
                        let hint = crate::validator::suggest(&key, KNOWN).map(|k| format!(" ; vouliez-vous dire `{}` ?", k)).unwrap_or_default();
                        return Err(format!(
                            "ligne {} : [config] : propriété `{}` inconnue{} (propriétés : {})",
                            self.last_line, key, hint, KNOWN.join(", ")
                        ));
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

    fn parse_figures_section(&mut self) -> Result<(Vec<StyleDef>, Vec<ParamDef>, Vec<FigureDef>), String> {
        let mut styles = Vec::new();
        let mut params = Vec::new();
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
                    "defparam" => {
                        self.advance();
                        params.push(self.parse_param(index)?);
                        index += 1;
                    }
                    "deffigure" => {
                        self.advance();
                        figures.push(self.parse_figure(index)?);
                        index += 1;
                    }
                    "defmodel" => {
                        self.advance();
                        self.parse_model()?;
                    }
                    "deffunction" => {
                        self.advance();
                        let name = self.parse_function()?;
                        let taken = figures.iter().any(|f: &FigureDef| f.name == name)
                            || styles.iter().any(|s: &StyleDef| s.name == name)
                            || params.iter().any(|p: &ParamDef| p.name == name);
                        if taken {
                            return Err(format!("ligne {} : deffunction {} : ce nom est déjà pris par un élément défini plus haut", self.last_line, name));
                        }
                        self.functions.push(name);
                    }
                    "defrepeat" => {
                        self.advance();
                        let group = self.parse_repeat(&mut index)?;
                        figures.extend(group);
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

        // Une fonction et une figure (ou un paramètre) ne peuvent pas porter le même nom :
        // `r(1, 2)` serait ambigu entre un point du repère r et un appel de fonction
        for f in &self.functions {
            if let Some(fig) = figures.iter().find(|g| &g.name == f) {
                return Err(format!("ligne {} : '{}' est déjà le nom d'une fonction (deffunction) : choisir un autre nom", fig.line, f));
            }
            if let Some(p) = params.iter().find(|p| &p.name == f) {
                return Err(format!("ligne {} : '{}' est déjà le nom d'une fonction (deffunction) : choisir un autre nom", p.line, f));
            }
        }

        Ok((styles, params, figures))
    }

    /// deffunction f(x, y) = corps : lue sur la ligne du fichier ; la fonction est utilisable
    /// dans toutes les formules qui suivent
    fn parse_function(&mut self) -> Result<String, String> {
        const FORM: &str = "deffunction f(x) = x^2 - 1  (plusieurs arguments : f(x, y))";
        let line = self.last_line;
        let text = self.source_lines.get(line.wrapping_sub(1)).cloned().ok_or_else(|| format!("ligne {} : deffunction : source introuvable", line))?;
        let after = text.trim_start().strip_prefix("deffunction").unwrap_or("").trim();
        let after = match after.find(" #") {
            Some(i) if !after[i..].starts_with(" #{") => &after[..i],
            _ => after,
        };
        let (head, body) = after.split_once('=').ok_or_else(|| format!("ligne {} : attendu {}", line, FORM))?;
        let head = head.trim();
        let (name, args) = head
            .strip_suffix(')')
            .and_then(|h| h.split_once('('))
            .ok_or_else(|| format!("ligne {} : attendu {}", line, FORM))?;
        let name = name.trim().to_string();
        let ok_name = |n: &str| !n.is_empty() && n.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok_name(&name) {
            return Err(format!("ligne {} : nom de fonction invalide '{}' : {}", line, name, FORM));
        }
        let args: Vec<String> = args.split(',').map(|a| a.trim().to_string()).collect();
        if args.iter().any(|a| !ok_name(a)) {
            return Err(format!("ligne {} : arguments invalides '{}' : {}", line, args.join(", "), FORM));
        }
        let body = body.trim();
        if body.is_empty() {
            return Err(format!("ligne {} : deffunction {} : corps vide : {}", line, name, FORM));
        }
        crate::renderer::expr::define_function(&name, &args, body, &self.param_names)
            .map_err(|e| format!("ligne {} : deffunction {}", line, e))?;
        while self.line() == line && *self.peek() != Token::EOF {
            self.advance();
        }
        Ok(name)
    }

    /// Texte d'une ligne du fichier, après le premier `=`
    fn line_after_equals(&self, line: usize) -> Result<String, String> {
        let text = self
            .source_lines
            .get(line.wrapping_sub(1))
            .and_then(|l| l.split_once('='))
            .map(|(_, rest)| rest)
            .ok_or_else(|| format!("ligne {} : ligne illisible", line))?;
        // Commentaire de fin de ligne : un # hors des parenthèses et des guillemets
        // (dans les arguments, c'est une couleur)
        let (mut depth, mut quoted) = (0i32, false);
        for (i, c) in text.char_indices() {
            match c {
                '"' => quoted = !quoted,
                '(' if !quoted => depth += 1,
                ')' if !quoted => depth -= 1,
                '#' if !quoted && depth == 0 => return Ok(text[..i].trim().to_string()),
                _ => {}
            }
        }
        Ok(text.trim().to_string())
    }

    /// Passe tous les tokens jusqu'à la ligne `last` comprise
    fn skip_through_line(&mut self, last: usize) {
        while self.line() <= last && *self.peek() != Token::EOF {
            self.advance();
        }
    }

    /// defmodel nom(a, b) =
    ///     type : ...
    /// Les lignes plus indentées que `defmodel` forment le corps, gardé comme texte.
    fn parse_model(&mut self) -> Result<(), String> {
        let line = self.last_line;
        if self.source_lines.is_empty() {
            return Err(format!("ligne {} : defmodel a besoin du texte source", line));
        }
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        loop {
            match self.advance() {
                Token::RParen => break,
                Token::Comma => {}
                Token::Ident(p) => params.push(p),
                tok => return Err(format!("ligne {} : defmodel {} : nom d'argument attendu, trouvé {:?}", line, name, tok)),
            }
        }
        self.expect(&Token::Equals)?;

        let indent = |l: &str| l.len() - l.trim_start().len();
        let def_indent = indent(&self.source_lines[line - 1]);
        let mut body = Vec::new();
        let mut last = line;
        for (i, l) in self.source_lines.iter().enumerate().skip(line) {
            if !l.trim().is_empty() && indent(l) <= def_indent {
                break;
            }
            body.push(l.clone());
            last = i + 1;
        }
        while body.last().is_some_and(|l: &String| l.trim().is_empty()) {
            body.pop();
        }
        if body.is_empty() {
            return Err(format!("ligne {} : le modèle '{}' est vide (ses lignes doivent être indentées)", line, name));
        }
        self.skip_through_line(last);
        if self.models.insert(name.clone(), Model { params, body }).is_some() {
            return Err(format!("ligne {} : le modèle '{}' est défini deux fois", line, name));
        }
        Ok(())
    }

    /// Crée une figure à partir d'un modèle : le texte obtenu est relu comme une figure ordinaire.
    /// Toutes ses propriétés portent la ligne de l'appel.
    fn figure_from_model(&self, name: &str, call: &str, index: usize, line: usize) -> Result<FigureDef, String> {
        let (model_name, args) =
            model::split_call(call).ok_or_else(|| format!("ligne {} : appel de modèle attendu : nom(arguments)", line))?;
        let m = self
            .models
            .get(model_name)
            .ok_or_else(|| format!("ligne {} : modèle inconnu '{}' (à définir plus haut avec defmodel)", line, model_name))?;
        let text = model::expand(model_name, m, name, &model::split_args(args)).map_err(|e| format!("ligne {} : {}", line, e))?;

        let mut sub = Parser::new(crate::lexer::tokenize(&text));
        sub.source_lines = text.lines().map(|l| l.to_string()).collect();
        // Les paramètres définis plus haut sont utilisables dans un modèle
        sub.param_names = self.param_names.clone();
        sub.advance(); // deffigure
        let mut fig = sub.parse_figure(index).map_err(|e| format!("ligne {} : modèle '{}' : {}", line, model_name, e))?;
        fig.line = line;
        for l in fig.prop_lines.values_mut() {
            *l = line;
        }
        Ok(fig)
    }

    /// defrepeat barres = barre(@i, red) for i in 0..7
    /// Crée barres_0 … barres_7 et le groupe `barres` qui les contient.
    fn parse_repeat(&mut self, index: &mut usize) -> Result<Vec<FigureDef>, String> {
        const FORM: &str = "defrepeat nom = modele(arguments) for i in 0..7";
        let line = self.last_line;
        if self.source_lines.is_empty() {
            return Err(format!("ligne {} : defrepeat a besoin du texte source", line));
        }
        let name = self.expect_ident()?;
        let text = self.line_after_equals(line)?;
        self.skip_through_line(line);
        let (call, range) = text.rsplit_once(" for ").ok_or_else(|| format!("ligne {} : forme attendue : {}", line, FORM))?;
        let (var, first, last) = model::parse_range(range).ok_or_else(|| format!("ligne {} : forme attendue : {}", line, FORM))?;
        if first < 0 || last < first || last - first >= 1000 {
            return Err(format!("ligne {} : defrepeat : intervalle {}..{} invalide (entiers ≥ 0, 1000 au plus)", line, first, last));
        }

        let mut figures = Vec::new();
        let mut items = Vec::new();
        for i in first..=last {
            let values = HashMap::from([(var.clone(), i.to_string())]);
            let call = model::substitute(call, &values).map_err(|e| format!("ligne {} : defrepeat : {}", line, e))?;
            let item = format!("{}_{}", name, i);
            figures.push(self.figure_from_model(&item, &call, *index, line)?);
            *index += 1;
            items.push(item);
        }
        let mut properties = HashMap::new();
        properties.insert("type".to_string(), PropValue::Single("group".to_string()));
        properties.insert("items".to_string(), PropValue::List(items));
        figures.push(FigureDef { name, index: *index, line, properties, prop_lines: HashMap::new() });
        *index += 1;
        Ok(figures)
    }

    /// defparam k = 1
    fn parse_param(&mut self, index: usize) -> Result<ParamDef, String> {
        let line = self.last_line; // ligne du mot-clé defparam
        let name = self.expect_ident()?;
        self.expect(&Token::Equals)?;
        match self.advance() {
            Token::Number(value) => {
                self.param_names.push(name.clone());
                Ok(ParamDef { name, index, line, value })
            }
            tok => Err(format!("ligne {} : defparam {} : valeur de départ numérique attendue, trouvé {:?}", line, name, tok)),
        }
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
                } else if let Some(v) = self.raw_call_value(line) {
                    properties.insert(key, v);
                } else if let Some(v) = self.raw_expr_value(line).or_else(|| self.raw_list_value(line)) {
                    properties.insert(key, v);
                } else if let Some(v) = self.raw_tuple_value(line) {
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

        // deffigure t1 = modele(arguments)
        if self.line() == line && matches!(self.peek(), Token::Ident(_)) && !self.source_lines.is_empty() {
            let call = self.line_after_equals(line)?;
            self.skip_through_line(line);
            return self.figure_from_model(&name, &call, index, line);
        }

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
                        None => match self.raw_call_value(line) {
                            Some(v) => v,
                            None => match self.raw_expr_value(line).or_else(|| self.raw_list_value(line)) {
                                Some(v) => v,
                                None => match self.raw_tuple_value(line) {
                                    Some(v) => v,
                                    None => self.parse_prop_value()?,
                                },
                            },
                        },
                    },
                };
                self.check_line_consumed(&key, line)?;
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
                Ok(PropValue::Tuple(self.parse_items(Token::RParen)?))
            }

            // Liste [a, b, c] ou liste de points [(20%, 80%), (50%, 20%)]
            Token::LBracket => {
                self.advance();
                Ok(PropValue::List(self.parse_items(Token::RBracket)?))
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
                    if matches!(unit.as_str(), "px" | "s" | "ms" | "deg" | "rad") {
                        let val = format!("{}{}", n, unit);
                        self.advance();
                        return Ok(PropValue::Single(val));
                    }
                    if let Some(msg) = unsupported_unit(&unit) {
                        return Err(format!("ligne {} : {}{} : {}", self.line(), n, unit, msg));
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
                            Token::EOF => return Err(self.unclosed(")")),
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
                Token::RBrace => {
                    self.advance();
                    break;
                }
                Token::EOF => return Err(format!("ligne {} : accolade `}}` manquante : le bloc ouvert à la ligne {} n'est pas fermé", self.last_line, line)),
                Token::Ident(key) => {
                    let action_line = self.line();
                    self.advance();
                    self.expect(&Token::Colon)?;

                    let action = match key.as_str() {
                        "start"   => TimelineAction::Start(self.parse_start_list()?),
                        "end"     => TimelineAction::End(self.parse_end_list()?),
                        "destroy" => TimelineAction::Destroy(self.parse_ident_array()?),
                        "anim"    => TimelineAction::Anim(self.parse_anim_list()?),
                        // `comment : none` efface le sous-titre (liste vide)
                        "comment" if matches!(self.peek(), Token::Ident(s) if s == "none") => {
                            self.advance();
                            TimelineAction::Comment(Vec::new())
                        }
                        // Une seule langue : `comment : "Texte"` suffit
                        "comment" if matches!(self.peek(), Token::StringLit(_)) => {
                            let Token::StringLit(s) = self.advance() else { unreachable!() };
                            TimelineAction::Comment(vec![s])
                        }
                        "comment" => TimelineAction::Comment(self.parse_string_list()?),
                        _ => {
                            const KNOWN: &[&str] = &["start", "end", "destroy", "anim", "comment"];
                            let hint = crate::validator::suggest(&key, KNOWN).map(|k| format!(" ; vouliez-vous dire `{}` ?", k)).unwrap_or_default();
                            return Err(format!("ligne {} : action `{}` inconnue{} (actions : {})", action_line, key, hint, KNOWN.join(", ")));
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
                Token::RBracket => { self.advance(); break; }
                Token::EOF => return Err(self.unclosed("]")),
                Token::Comma => { self.advance(); }
                Token::Ident(s) => {
                    self.advance();
                    if s == "audio" {
                        // audio nom_fichier.mp3 [volume=x] [loop], dans n'importe quel ordre
                        let file = self.parse_ident_or_string()?;
                        let mut volume = None;
                        let mut looping = false;
                        while let Token::Ident(k) = self.peek().clone() {
                            match k.as_str() {
                                "volume" => {
                                    self.advance();
                                    self.expect(&Token::Equals)?;
                                    if let Token::Number(v) = self.advance() {
                                        volume = Some(v);
                                    }
                                }
                                "loop" => {
                                    self.advance();
                                    looping = true;
                                }
                                _ => break,
                            }
                        }
                        items.push(StartItem::Audio { file, volume, looping });
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
                Token::RBracket => { self.advance(); break; }
                Token::EOF => return Err(self.unclosed("]")),
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
                Token::RBracket => { self.advance(); break; }
                Token::EOF => return Err(self.unclosed("]")),
                Token::Comma => { self.advance(); }
                Token::Ident(figure) => {
                    self.advance();
                    let mut calls = Vec::new();
                    if *self.peek() == Token::LBracket {
                        self.advance();
                        loop {
                            match self.peek().clone() {
                                Token::RBracket => { self.advance(); break; }
                                Token::EOF => return Err(self.unclosed("]")),
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
                    Token::RParen => { self.advance(); break; }
                    Token::EOF => return Err(self.unclosed(")")),
                    Token::Comma => { self.advance(); }
                    Token::Ident(key) if *self.peek_at(1) == Token::Equals => {
                        // Paramètre nommé : to=..., path=..., ease=...
                        self.advance();
                        self.advance();
                        let val = self.parse_anim_arg();
                        params.push(format!("{}={}", key, val));
                    }
                    _ => {
                        let val = self.parse_anim_arg();
                        if !val.is_empty() {
                            params.push(val);
                        }
                    }
                }
            }
        }

        Ok(AnimCall { name, params, line })
    }

    /// Un argument d'animation en entier, jusqu'à la virgule ou la parenthèse fermante :
    /// `2s`, `(50%, 50%)`, `center(b)`, `2*pi`, `-45`. Rien n'est laissé de côté : un calcul
    /// est gardé tel quel (et calculé ou refusé plus loin), jamais tronqué.
    fn parse_anim_arg(&mut self) -> String {
        let mut out = String::new();
        let mut depth = 0;
        loop {
            match self.peek().clone() {
                Token::EOF => break,
                Token::Comma | Token::RParen if depth == 0 => break,
                Token::LParen => { depth += 1; out.push('('); }
                Token::RParen => { depth -= 1; out.push(')'); }
                Token::Comma => out.push(','),
                Token::Number(n) => out.push_str(&n.to_string()),
                Token::Percentage(p) => out.push_str(&p),
                Token::Ident(s) | Token::StringLit(s) => out.push_str(&s),
                Token::Op(c) => out.push(c),
                Token::Pipe => out.push('|'),
                Token::Equals => out.push('='),
                _ => {}
            }
            self.advance();
        }
        out
    }

    /// Éléments d'une liste ou d'un couple, jusqu'à la fermeture `close` (déjà ouverte).
    /// Chaque élément est gardé en entier, sans espaces : `2*pi`, `(20%,80%)`, `repere(1,2)`,
    /// `-x/2`. Rien n'est laissé de côté : un calcul est calculé ou refusé plus loin.
    fn parse_items(&mut self, close: Token) -> Result<Vec<String>, String> {
        let mut items = Vec::new();
        let mut cur = String::new();
        let mut depth = 0usize;
        // Deux mots ou deux nombres qui se suivent sans virgule restent séparés (signalés plus loin)
        let mut last_word = false;
        loop {
            let tok = self.peek().clone();
            if depth == 0 && tok == close {
                self.advance();
                break;
            }
            let word = matches!(tok, Token::Number(_) | Token::Ident(_) | Token::Percentage(_) | Token::StringLit(_));
            let glue = last_word && (matches!(tok, Token::Number(_) | Token::Percentage(_)) || (matches!(tok, Token::Ident(_)) && !cur.ends_with(|c: char| c.is_ascii_digit())));
            if glue {
                cur.push(' ');
            }
            match tok {
                Token::EOF => return Err(self.unclosed(if close == Token::RParen { ")" } else { "]" })),
                Token::Comma if depth == 0 => {
                    if !cur.is_empty() {
                        items.push(std::mem::take(&mut cur));
                    }
                }
                Token::Comma => cur.push(','),
                Token::LParen => { depth += 1; cur.push('('); }
                Token::LBracket => { depth += 1; cur.push('['); }
                Token::RParen => { depth = depth.saturating_sub(1); cur.push(')'); }
                Token::RBracket => { depth = depth.saturating_sub(1); cur.push(']'); }
                Token::Number(n) => cur.push_str(&n.to_string()),
                Token::Percentage(p) => cur.push_str(&p),
                Token::Ident(s) | Token::StringLit(s) => cur.push_str(&s),
                Token::Op(c) => cur.push(c),
                Token::Pipe => cur.push('|'),
                Token::Equals => cur.push('='),
                _ => {}
            }
            last_word = word;
            self.advance();
        }
        if !cur.is_empty() {
            items.push(cur);
        }
        Ok(items)
    }

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
                Token::EOF => return Err(self.unclosed("]")),
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
                Token::EOF => return Err(self.unclosed("]")),
                Token::Comma => { self.advance(); }
                Token::Ident(s) => { items.push(s); self.advance(); }
                _ => { self.advance(); }
            }
        }
        Ok(items)
    }

}
/// Unités refusées, avec ce qu'il faut écrire à la place
pub fn unsupported_unit(unit: &str) -> Option<String> {
    matches!(unit, "cm" | "mm" | "pt" | "em" | "in").then(|| {
        format!("unité '{}' non prise en charge (unités : %, px, deg, rad, s, ms) : utiliser % de l'écran ou px", unit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn config_inconnue_refusee() {
        let src = "[config]\nbackgrund : red\n[figures]\n[timeline]\n";
        let err = parse_with_source(tokenize(src), src).unwrap_err();
        assert!(err.contains("`backgrund` inconnue ; vouliez-vous dire `background` ?"), "{}", err);
    }

    #[test]
    fn parametres_dans_un_modele() {
        let src = "[figures]\ndefparam n = 3\ndefmodel m(i) =\n    type    : circle\n    center  : (50%, 50%)\n    radius  : (2*n)%\n    opacity : @i < n\ndefrepeat c = m(@i) for i in 0..3\n[timeline]\n";
        let doc = parse_with_source(crate::lexer::tokenize(src), src).unwrap();
        let f = doc.figures.iter().find(|f| f.name == "c_2").unwrap();
        assert_eq!(f.properties.get("opacity"), Some(&PropValue::Single("2<n".into())));
        assert_eq!(f.properties.get("radius"), Some(&PropValue::Single("(2*n)%".into())));
    }

    #[test]
    fn formule_qui_commence_par_une_barre() {
        let src = "[figures]\ndeffigure e =\n    type : equation\n    expr : |x| + 1\n    pos  : (50%, 50%)\n[timeline]\n";
        let doc = parse_with_source(crate::lexer::tokenize(src), src).unwrap();
        assert_eq!(doc.figures[0].properties.get("expr"), Some(&PropValue::Single("|x| + 1".into())));
        // Un vrai bloc reste un bloc
        let src = "[figures]\ndeffigure t =\n    type : latex\n    content : |\n        a\n        b\n[timeline]\n";
        let doc = parse_with_source(crate::lexer::tokenize(src), src).unwrap();
        assert_eq!(doc.figures[0].properties.get("content"), Some(&PropValue::Block("a\nb".into())));
    }

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
