use crate::error::Result;
use regex::Regex;
use std::collections::HashMap;

/// Represents different types of AST nodes
#[derive(Debug, Clone, PartialEq)]
pub enum AstNodeType {
    // Simple nodes (no body)
    FunctionCall,
    VariableDeclaration,
    Assignment,
    Literal,
    Identifier,
    BinaryOperator,
    UnaryOperator,

    // Compound nodes (have body/children)
    IfStatement,
    ForLoop,
    WhileLoop,
    Block,
    Function,
    Class,
    Namespace,
}

/// Represents a node in the AST
#[derive(Debug, Clone)]
pub struct AstNode {
    pub node_type: AstNodeType,
    pub content: String,
    pub line_number: usize,
    pub column_start: usize,
    #[allow(dead_code)]
    pub column_end: usize,
    pub children: Vec<AstNode>,
}

impl AstNode {
    pub fn new(
        node_type: AstNodeType,
        content: String,
        line_number: usize,
        column_start: usize,
        column_end: usize,
    ) -> Self {
        Self {
            node_type,
            content,
            line_number,
            column_start,
            column_end,
            children: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn add_child(&mut self, child: AstNode) {
        self.children.push(child);
    }

    pub fn is_simple(&self) -> bool {
        matches!(
            self.node_type,
            AstNodeType::FunctionCall
                | AstNodeType::VariableDeclaration
                | AstNodeType::Assignment
                | AstNodeType::Literal
                | AstNodeType::Identifier
                | AstNodeType::BinaryOperator
                | AstNodeType::UnaryOperator
        )
    }

    #[allow(dead_code)]
    pub fn is_compound(&self) -> bool {
        !self.is_simple()
    }
}

/// Expert knowledge for detecting arid nodes
pub struct ExpertKnowledge {
    arid_function_patterns: Vec<Regex>,
    arid_variable_patterns: Vec<Regex>,
    arid_statement_patterns: Vec<Regex>,
    arid_namespace_patterns: Vec<Regex>,
}

impl ExpertKnowledge {
    pub fn new() -> Result<Self> {
        let arid_function_patterns = vec![
            // Memory management functions
            Regex::new(r"std::vector<.*>::reserve")?,
            Regex::new(r"std::vector<.*>::resize")?,
            Regex::new(r"std::.*::reserve")?,
            Regex::new(r"\.reserve\s*\(")?,
            Regex::new(r"\.resize\s*\(")?,
            // I/O operations (typically not unit tested)
            Regex::new(r"std::cout\s*<<")?,
            Regex::new(r"std::cerr\s*<<")?,
            Regex::new(r"printf\s*\(")?,
            Regex::new(r"fprintf\s*\(")?,
            Regex::new(r"std::endl")?,
            // Logging functions - note the patterns match anywhere in the string
            Regex::new(r"LogPrintf\s*\(")?,
            Regex::new(r"LogPrint\s*\(")?,
            Regex::new(r"LogDebug\s*\(")?,
            Regex::new(r"\blog\.")?,
            Regex::new(r"\blogger\.")?,
            Regex::new(r"\blogging\.")?,
            // Debug/trace functions
            Regex::new(r"assert\s*\(")?,
            Regex::new(r"DEBUG_")?,
            Regex::new(r"TRACE_")?,
            // Bitcoin Core specific patterns
            Regex::new(r"G_FUZZING")?,
            Regex::new(r"fPrintToConsole")?,
            Regex::new(r"strprintf\s*\(")?,
            // Memory allocation that's usually not tested
            Regex::new(r"malloc\s*\(")?,
            Regex::new(r"calloc\s*\(")?,
            Regex::new(r"realloc\s*\(")?,
            Regex::new(r"free\s*\(")?,
            // Thread/concurrency primitives often not unit tested
            Regex::new(r"std::thread")?,
            Regex::new(r"std::mutex")?,
            Regex::new(r"std::lock_guard")?,
            // Performance monitoring (usually not tested)
            Regex::new(r"\.now\(\)")?,
            Regex::new(r"steady_clock")?,
            Regex::new(r"high_resolution_clock")?,
        ];

        let arid_variable_patterns = vec![
            // Timing/performance variables
            Regex::new(r".*_time$")?,
            Regex::new(r".*_duration$")?,
            Regex::new(r".*_start$")?,
            Regex::new(r".*_end$")?,
            // Debug/logging variables
            Regex::new(r".*_debug$")?,
            Regex::new(r".*_log$")?,
            Regex::new(r".*_trace$")?,
            // Temporary/scratch variables
            Regex::new(r"temp_.*")?,
            Regex::new(r"tmp_.*")?,
            Regex::new(r"scratch_.*")?,
        ];

        let arid_statement_patterns = vec![
            // Comments
            Regex::new(r"^\s*//")?,
            Regex::new(r"^\s*/\*")?,
            // Preprocessor directives
            Regex::new(r"^\s*#")?,
            // Empty statements
            Regex::new(r"^\s*;")?,
            // Namespace declarations
            Regex::new(r"^\s*namespace\s+")?,
            Regex::new(r"^\s*using\s+namespace\s+")?,
            // Forward declarations
            Regex::new(r"^\s*class\s+\w+\s*;")?,
            Regex::new(r"^\s*struct\s+\w+\s*;")?,
        ];

        let arid_namespace_patterns = vec![
            // Standard library
            Regex::new(r"std::")?,
            // Boost library (often infrastructure)
            Regex::new(r"boost::")?,
            // Testing frameworks
            Regex::new(r"testing::")?,
            Regex::new(r"gtest::")?,
        ];

        Ok(Self {
            arid_function_patterns,
            arid_variable_patterns,
            arid_statement_patterns,
            arid_namespace_patterns,
        })
    }

    /// Expert function that determines if a simple node is arid
    pub fn is_arid_simple_node(&self, node: &AstNode) -> bool {
        if !node.is_simple() {
            return false;
        }

        let content = &node.content;

        // Check function call patterns first (most specific)
        if matches!(node.node_type, AstNodeType::FunctionCall) {
            for pattern in &self.arid_function_patterns {
                if pattern.is_match(content) {
                    return true;
                }
            }
        }

        // Check variable patterns
        if matches!(
            node.node_type,
            AstNodeType::VariableDeclaration | AstNodeType::Assignment
        ) {
            for pattern in &self.arid_variable_patterns {
                if pattern.is_match(content) {
                    return true;
                }
            }
        }

        // Check general statement patterns
        for pattern in &self.arid_statement_patterns {
            if pattern.is_match(content) {
                return true;
            }
        }

        // Check namespace patterns (but not for function calls as that's too broad)
        if !matches!(node.node_type, AstNodeType::FunctionCall) {
            for pattern in &self.arid_namespace_patterns {
                if pattern.is_match(content) {
                    return true;
                }
            }
        }

        false
    }
}

/// Arid node detector implementing Google's algorithm
pub struct AridNodeDetector {
    expert: ExpertKnowledge,
    cache: HashMap<String, bool>,
}

impl AridNodeDetector {
    pub fn new() -> Result<Self> {
        Ok(Self {
            expert: ExpertKnowledge::new()?,
            cache: HashMap::new(),
        })
    }

    /// Implementation of Google's arid node detection algorithm
    /// arid(N) = expert(N) if simple(N)
    ///         = 1 if ∀(arid(c)) = 1, ∀c ∈ N otherwise
    pub fn is_arid(&mut self, node: &AstNode) -> bool {
        // Create cache key
        let cache_key = format!(
            "{}:{}:{}",
            node.line_number, node.column_start, node.content
        );

        if let Some(&cached_result) = self.cache.get(&cache_key) {
            return cached_result;
        }

        let result = if node.is_simple() {
            // For simple nodes, use expert knowledge
            self.expert.is_arid_simple_node(node)
        } else {
            // For compound nodes, check if ALL children are arid
            if node.children.is_empty() {
                // Empty compound node is not arid
                false
            } else {
                // All children must be arid for compound node to be arid
                node.children.iter().all(|child| self.is_arid(child))
            }
        };

        // Cache the result
        self.cache.insert(cache_key, result);
        result
    }

    /// Context-aware version that checks if a line should be mutated
    /// Takes all lines and the current line index to understand control structures
    pub fn should_mutate_line_with_context(
        &mut self,
        lines: &[String],
        line_index: usize,
    ) -> bool {
        let line = &lines[line_index];
        let trimmed = line.trim();

        // Skip empty lines and closing braces
        if trimmed.is_empty() || trimmed == "}" {
            return false;
        }

        let line_number = line_index + 1;
        let node_type = self.classify_line(trimmed);

        // For control structures, check if their body is all arid
        if matches!(
            node_type,
            AstNodeType::IfStatement | AstNodeType::ForLoop | AstNodeType::WhileLoop
        ) {
            // If the control structure body is all arid, don't mutate the control structure
            return !self.is_control_structure_body_arid(lines, line_index);
        }

        // For lines inside control structures, we still need to check them individually
        // unless they're part of an all-arid control structure (which is handled above)
        let node = self.parse_line_to_simple_ast(trimmed, line_number);
        !self.is_arid(&node)
    }

    /// Check if a control structure's body contains only arid statements
    fn is_control_structure_body_arid(&mut self, lines: &[String], start_index: usize) -> bool {
        let start_line = lines[start_index].trim();

        // Check if this is a single-line control structure (no braces)
        // e.g., "if (condition) single_statement;"
        if !start_line.contains('{') {
            // Look for the statement on the same line or next line
            let statement = if start_line.contains(')') && start_line.ends_with(';') {
                // Extract everything after the closing paren
                if let Some(pos) = start_line.rfind(')') {
                    start_line[pos + 1..].trim()
                } else {
                    start_line
                }
            } else if start_index + 1 < lines.len() {
                // Statement is on the next line
                lines[start_index + 1].trim()
            } else {
                return false;
            };

            // Parse and check if the statement is arid
            let node = self.parse_line_to_simple_ast(statement, start_index + 2);
            return self.is_arid(&node);
        }

        // Find the opening brace
        let mut brace_line_index = start_index;
        if !start_line.contains('{') {
            // Opening brace might be on the next line
            brace_line_index = start_index + 1;
            if brace_line_index >= lines.len() || !lines[brace_line_index].contains('{') {
                return false;
            }
        }

        // Find matching closing brace
        let body_range = match self.find_matching_brace(lines, brace_line_index) {
            Some(end_index) => (brace_line_index + 1, end_index),
            None => return false,
        };

        // Check if all non-empty lines in the body are arid
        let mut has_non_empty_line = false;
        for i in body_range.0..body_range.1 {
            let line = lines[i].trim();

            // Skip empty lines and braces
            if line.is_empty() || line == "{" || line == "}" {
                continue;
            }

            has_non_empty_line = true;

            // Parse the line and check if it's arid
            let node = self.parse_line_to_simple_ast(line, i + 1);
            if !self.is_arid(&node) {
                // Found a non-arid line in the body
                return false;
            }
        }

        // If we found at least one non-empty line and all were arid, return true
        // If no non-empty lines, return false (empty body is not arid)
        has_non_empty_line
    }

    /// Find the index of the closing brace that matches the opening brace at start_index
    fn find_matching_brace(&self, lines: &[String], start_index: usize) -> Option<usize> {
        let mut brace_count = 0;
        let mut found_opening = false;

        for (i, line) in lines.iter().enumerate().skip(start_index) {
            for ch in line.chars() {
                match ch {
                    '{' => {
                        brace_count += 1;
                        found_opening = true;
                    }
                    '}' => {
                        brace_count -= 1;
                        if found_opening && brace_count == 0 {
                            return Some(i);
                        }
                    }
                    _ => {}
                }
            }
        }

        None
    }

    /// Simple heuristic-based parsing to create AST nodes from single lines
    fn parse_line_to_simple_ast(&self, line_content: &str, line_number: usize) -> AstNode {
        let trimmed = line_content.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
            return AstNode::new(
                AstNodeType::Identifier,
                trimmed.to_string(),
                line_number,
                0,
                line_content.len(),
            );
        }

        // Determine node type based on content patterns
        let node_type = self.classify_line(trimmed);

        AstNode::new(
            node_type,
            trimmed.to_string(),
            line_number,
            0,
            line_content.len(),
        )
    }

    /// Classify a line of code into the appropriate AST node type
    fn classify_line(&self, line: &str) -> AstNodeType {
        // Namespace declarations
        if line.starts_with("namespace ") || line.contains("using namespace") {
            return AstNodeType::Namespace;
        }

        // Class declarations
        if line.starts_with("class ") || line.starts_with("struct ") {
            return AstNodeType::Class;
        }

        // Control flow statements (compound nodes) - check these before function declarations
        if line.starts_with("if ") || line.starts_with("if(") || line.contains("} else ") {
            return AstNodeType::IfStatement;
        }
        if line.starts_with("for ") || line.starts_with("for(") {
            return AstNodeType::ForLoop;
        }
        if line.starts_with("while ") || line.starts_with("while(") {
            return AstNodeType::WhileLoop;
        }

        // Block statements
        if line == "{" || line == "}" || line.ends_with(" {") {
            return AstNodeType::Block;
        }

        // Variable declarations
        if self.is_variable_declaration(line) {
            return AstNodeType::VariableDeclaration;
        }

        // Assignment operations
        if self.is_assignment(line) {
            return AstNodeType::Assignment;
        }

        // Function calls - check BEFORE function declarations
        if self.is_function_call(line) {
            return AstNodeType::FunctionCall;
        }

        // Function declarations/definitions - check AFTER function calls
        if self.is_function_declaration(line) {
            return AstNodeType::Function;
        }

        // Binary operators
        if self.is_binary_operation(line) {
            return AstNodeType::BinaryOperator;
        }

        // Unary operators
        if self.is_unary_operation(line) {
            return AstNodeType::UnaryOperator;
        }

        // Literals
        if self.is_literal(line) {
            return AstNodeType::Literal;
        }

        // Default to identifier
        AstNodeType::Identifier
    }

    /// Check if line is a function declaration or definition
    fn is_function_declaration(&self, line: &str) -> bool {
        // Function calls end with ); - those are NOT declarations
        if line.trim().ends_with(");") {
            return false;
        }

        // Function declarations typically:
        // - Have a return type before the function name
        // - End with { or just ; (not );)
        // - Have modifiers like virtual, static, etc.

        let function_patterns = [
            // Return type + function name + params + opening brace
            Regex::new(r"^\s*\w+\s+\w+\s*\([^)]*\)\s*\{").unwrap(),
            // Constructor/destructor with opening brace or initializer list
            Regex::new(r"^\s*~?\w+\s*\([^)]*\)\s*[{:]").unwrap(),
            // Template function
            Regex::new(r"^\s*template\s*<[^>]*>").unwrap(),
            // Function with qualifiers (virtual, static, inline, explicit, etc.)
            Regex::new(r"^\s*(?:virtual\s+|static\s+|inline\s+|explicit\s+)").unwrap(),
            // Return type + function name + params + ending semicolon (forward declaration)
            // But make sure it doesn't end with );
            Regex::new(r"^\s*\w+\s+\w+\s*\([^)]*\)\s*;\s*$").unwrap(),
        ];

        function_patterns
            .iter()
            .any(|pattern| pattern.is_match(line))
            && !line.contains('=')
    }

    /// Check if line is a variable declaration
    fn is_variable_declaration(&self, line: &str) -> bool {
        let var_patterns = [
            Regex::new(r"^\s*(int|bool|char|float|double|long|short|unsigned|signed)\s+\w+")
                .unwrap(),
            Regex::new(r"^\s*std::\w+\s*<?[^>]*>?\s+\w+").unwrap(),
            Regex::new(r"^\s*[A-Z]\w*\s+\w+").unwrap(),
            Regex::new(r"^\s*\w+\s*[*&]+\s*\w+").unwrap(),
            Regex::new(r"^\s*const\s+\w+").unwrap(),
            Regex::new(r"^\s*auto\s+\w+").unwrap(),
        ];

        var_patterns.iter().any(|pattern| pattern.is_match(line))
            && !line.contains('(')
            && (line.contains('=') || line.ends_with(';'))
    }

    /// Check if line is an assignment
    fn is_assignment(&self, line: &str) -> bool {
        line.contains('=')
            && !line.contains("==")
            && !line.contains("!=")
            && !line.contains("<=")
            && !line.contains(">=")
            && !self.is_variable_declaration(line)
    }

    /// Check if line is a function call
    fn is_function_call(&self, line: &str) -> bool {
        line.contains('(')
            && line.contains(')')
            && !self.is_function_declaration(line)
            && !self.is_variable_declaration(line)
            && !line.starts_with("if ")
            && !line.starts_with("if(")
            && !line.starts_with("while ")
            && !line.starts_with("while(")
            && !line.starts_with("for ")
            && !line.starts_with("for(")
    }

    /// Check if line contains binary operations
    fn is_binary_operation(&self, line: &str) -> bool {
        let binary_ops = [
            "+", "-", "*", "/", "%", "&&", "||", "&", "|", "^", "<<", ">>",
        ];
        binary_ops.iter().any(|op| line.contains(op)) && !line.contains('=') && !line.contains('(')
    }

    /// Check if line contains unary operations
    fn is_unary_operation(&self, line: &str) -> bool {
        let unary_patterns = [
            Regex::new(r"\+\+\w+").unwrap(),
            Regex::new(r"\w\+\+").unwrap(),
            Regex::new(r"--\w+").unwrap(),
            Regex::new(r"\w--").unwrap(),
            Regex::new(r"!\w+").unwrap(),
            Regex::new(r"~\w+").unwrap(),
        ];

        unary_patterns.iter().any(|pattern| pattern.is_match(line))
    }

    /// Check if line is a literal value
    fn is_literal(&self, line: &str) -> bool {
        let literal_patterns = [
            Regex::new(r"^\s*\d+\s*;?\s*$").unwrap(),
            Regex::new(r"^\s*\d+\.\d+\s*;?\s*$").unwrap(),
            Regex::new(r#"^\s*"[^"]*"\s*;?\s*$"#).unwrap(),
            Regex::new(r"^\s*'[^']*'\s*;?\s*$").unwrap(),
            Regex::new(r"^\s*(true|false)\s*;?\s*$").unwrap(),
            Regex::new(r"^\s*(nullptr|NULL)\s*;?\s*$").unwrap(),
        ];

        literal_patterns
            .iter()
            .any(|pattern| pattern.is_match(line))
    }

    /// Add a new expert rule at runtime
    pub fn add_expert_rule(&mut self, pattern: &str, description: &str) -> Result<()> {
        let regex = Regex::new(pattern)?;
        self.expert.arid_function_patterns.push(regex);
        println!("Added expert rule: {} ({})", pattern, description);
        Ok(())
    }

    /// Get statistics about arid node detection
    pub fn get_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        stats.insert(
            "total_expert_rules".to_string(),
            self.expert.arid_function_patterns.len()
                + self.expert.arid_variable_patterns.len()
                + self.expert.arid_statement_patterns.len(),
        );
        stats.insert("cache_size".to_string(), self.cache.len());
        stats.insert(
            "function_patterns".to_string(),
            self.expert.arid_function_patterns.len(),
        );
        stats.insert(
            "variable_patterns".to_string(),
            self.expert.arid_variable_patterns.len(),
        );
        stats.insert(
            "statement_patterns".to_string(),
            self.expert.arid_statement_patterns.len(),
        );
        stats
    }

    /// Export detailed analysis of which lines were filtered and why
    #[allow(dead_code)]
    pub fn analyze_file_detailed(&mut self, file_content: &str) -> DetailedAnalysis {
        let lines: Vec<String> = file_content.lines().map(|s| s.to_string()).collect();
        let mut analysis = DetailedAnalysis::new();

        for (idx, line) in lines.iter().enumerate() {
            let line_number = idx + 1;
            let should_mutate = self.should_mutate_line_with_context(&lines, idx);
            let node = self.parse_line_to_simple_ast(line, line_number);
            let is_arid = !should_mutate;
            let reason = if is_arid {
                self.get_arid_reason(&node, &lines, idx)
            } else {
                "Not arid - will be mutated".to_string()
            };

            analysis.add_line_analysis(LineAnalysis {
                line_number,
                content: line.to_string(),
                node_type: node.node_type,
                is_arid,
                reason,
            });
        }

        analysis
    }

    /// Get the reason why a node is considered arid
    #[allow(dead_code)]
    fn get_arid_reason(&self, node: &AstNode, _lines: &[String], _line_index: usize) -> String {
        // Check if this is a control structure with arid body
        if matches!(
            node.node_type,
            AstNodeType::IfStatement | AstNodeType::ForLoop | AstNodeType::WhileLoop
        ) {
            return "Control structure with arid body (logging/debugging only)".to_string();
        }

        if !node.is_simple() {
            return "Compound node - arid if all children are arid".to_string();
        }

        let content = &node.content;

        // Check function call patterns
        if matches!(node.node_type, AstNodeType::FunctionCall) {
            for (idx, pattern) in self.expert.arid_function_patterns.iter().enumerate() {
                if pattern.is_match(content) {
                    return format!(
                        "Matches arid function pattern #{}: {}",
                        idx + 1,
                        pattern.as_str()
                    );
                }
            }
        }

        // Check variable patterns
        if matches!(
            node.node_type,
            AstNodeType::VariableDeclaration | AstNodeType::Assignment
        ) {
            for (idx, pattern) in self.expert.arid_variable_patterns.iter().enumerate() {
                if pattern.is_match(content) {
                    return format!(
                        "Matches arid variable pattern #{}: {}",
                        idx + 1,
                        pattern.as_str()
                    );
                }
            }
        }

        // Check statement patterns
        for (idx, pattern) in self.expert.arid_statement_patterns.iter().enumerate() {
            if pattern.is_match(content) {
                return format!(
                    "Matches arid statement pattern #{}: {}",
                    idx + 1,
                    pattern.as_str()
                );
            }
        }

        "Not arid".to_string()
    }

    /// Clear the cache (useful for testing or when rules change)
    #[allow(dead_code)]
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

/// Detailed analysis results for a file
#[allow(dead_code)]
#[derive(Debug)]
pub struct DetailedAnalysis {
    pub lines: Vec<LineAnalysis>,
    pub summary: AnalysisSummary,
}

#[allow(dead_code)]
impl DetailedAnalysis {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            summary: AnalysisSummary::default(),
        }
    }

    pub fn add_line_analysis(&mut self, analysis: LineAnalysis) {
        if analysis.is_arid {
            self.summary.arid_lines += 1;
        } else {
            self.summary.mutatable_lines += 1;
        }
        self.summary.total_lines += 1;
        self.lines.push(analysis);
    }

    pub fn print_summary(&self) {
        println!("\n=== AST Analysis Summary ===");
        println!("Total lines: {}", self.summary.total_lines);
        println!("Mutatable lines: {}", self.summary.mutatable_lines);
        println!("Arid lines: {}", self.summary.arid_lines);
        println!(
            "Filtering efficiency: {:.1}% reduction",
            (self.summary.arid_lines as f64 / self.summary.total_lines as f64) * 100.0
        );
    }

    pub fn print_arid_lines(&self) {
        println!("\n=== Filtered Out (Arid) Lines ===");
        for line in &self.lines {
            if line.is_arid {
                println!(
                    "Line {}: {} | Reason: {}",
                    line.line_number,
                    line.content.trim(),
                    line.reason
                );
            }
        }
    }
}

/// Analysis of a single line
#[allow(dead_code)]
#[derive(Debug)]
pub struct LineAnalysis {
    pub line_number: usize,
    pub content: String,
    pub node_type: AstNodeType,
    pub is_arid: bool,
    pub reason: String,
}

/// Summary statistics for analysis
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct AnalysisSummary {
    pub total_lines: usize,
    pub mutatable_lines: usize,
    pub arid_lines: usize,
}

/// Integration with existing mutation system - context-aware version
pub fn filter_mutatable_lines(lines: &[String], detector: &mut AridNodeDetector) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(idx, _line)| {
            let line_number = idx + 1;
            if detector.should_mutate_line_with_context(lines, idx) {
                Some(line_number)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expert_knowledge() {
        let expert = ExpertKnowledge::new().unwrap();

        // Test arid function calls
        let reserve_node = AstNode::new(
            AstNodeType::FunctionCall,
            "vec.reserve(100)".to_string(),
            1,
            0,
            15,
        );
        assert!(expert.is_arid_simple_node(&reserve_node));

        // Test non-arid function calls
        let normal_node = AstNode::new(
            AstNodeType::FunctionCall,
            "calculate_sum(a, b)".to_string(),
            1,
            0,
            18,
        );
        assert!(!expert.is_arid_simple_node(&normal_node));

        // Test LogDebug function call
        let log_debug_node = AstNode::new(
            AstNodeType::FunctionCall,
            "LogDebug(BCLog::ADDRMAN, \"test\");".to_string(),
            1,
            0,
            30,
        );
        assert!(expert.is_arid_simple_node(&log_debug_node), "LogDebug should be recognized as arid");
    }

    #[test]
    fn test_arid_detection_algorithm() {
        let mut detector = AridNodeDetector::new().unwrap();

        // Test simple arid node
        let arid_simple = AstNode::new(
            AstNodeType::FunctionCall,
            "std::cout << \"debug\"".to_string(),
            1,
            0,
            20,
        );
        assert!(detector.is_arid(&arid_simple));

        // Test compound node with all arid children
        let mut compound_arid =
            AstNode::new(AstNodeType::Block, "{ debug block }".to_string(), 1, 0, 15);
        compound_arid.add_child(arid_simple.clone());
        assert!(detector.is_arid(&compound_arid));

        // Test compound node with non-arid child
        let non_arid_simple = AstNode::new(
            AstNodeType::FunctionCall,
            "important_function()".to_string(),
            2,
            0,
            20,
        );
        let mut compound_mixed =
            AstNode::new(AstNodeType::Block, "{ mixed block }".to_string(), 1, 0, 15);
        compound_mixed.add_child(arid_simple);
        compound_mixed.add_child(non_arid_simple);
        assert!(!detector.is_arid(&compound_mixed));
    }

    #[test]
    fn test_line_mutation_filtering() {
        let mut detector = AridNodeDetector::new().unwrap();

        let lines = vec![
            "int x = 5;".to_string(),              // Should mutate
            "std::cout << \"debug\";".to_string(), // Should NOT mutate (arid)
            "vec.reserve(100);".to_string(),       // Should NOT mutate (arid)
            "return x + y;".to_string(),           // Should mutate
        ];

        let mutatable_lines = filter_mutatable_lines(&lines, &mut detector);

        // Should only include lines 1 and 4
        assert_eq!(mutatable_lines, vec![1, 4]);
    }

    #[test]
    fn test_if_statement_with_logging() {
        let mut detector = AridNodeDetector::new().unwrap();

        let lines = vec![
            "if (!restore_bucketing) {".to_string(),
            "    LogDebug(BCLog::ADDRMAN, \"Bucketing method was updated, re-bucketing addrman entries from disk\\n\");".to_string(),
            "}".to_string(),
        ];

        // First, let's test that LogDebug itself is recognized as arid
        let log_line = lines[1].trim();
        let log_node = detector.parse_line_to_simple_ast(log_line, 2);
        assert_eq!(log_node.node_type, AstNodeType::FunctionCall, "LogDebug line should be classified as FunctionCall");
        assert!(detector.is_arid(&log_node), "LogDebug should be recognized as arid");

        let mutatable_lines = filter_mutatable_lines(&lines, &mut detector);

        // The if statement should NOT be mutated because it only contains logging
        // Lines 2 (LogDebug) and 3 (closing brace) also should not be mutated
        assert!(
            mutatable_lines.is_empty(),
            "Expected no mutatable lines, got: {:?}",
            mutatable_lines
        );
    }

    #[test]
    fn test_if_statement_with_non_arid_body() {
        let mut detector = AridNodeDetector::new().unwrap();

        let lines = vec![
            "if (condition) {".to_string(),
            "    x = x + 1;".to_string(),
            "}".to_string(),
        ];

        let mutatable_lines = filter_mutatable_lines(&lines, &mut detector);

        // The if statement and the assignment should be mutated
        assert!(
            mutatable_lines.contains(&1),
            "If statement should be mutatable"
        );
        assert!(
            mutatable_lines.contains(&2),
            "Assignment should be mutatable"
        );
    }

    #[test]
    fn test_if_statement_mixed_body() {
        let mut detector = AridNodeDetector::new().unwrap();

        let lines = vec![
            "if (condition) {".to_string(),
            "    LogDebug(BCLog::TEST, \"debug\");".to_string(),
            "    x = x + 1;".to_string(),
            "}".to_string(),
        ];

        let mutatable_lines = filter_mutatable_lines(&lines, &mut detector);

        // The if statement should be mutated because it has non-arid content
        assert!(
            mutatable_lines.contains(&1),
            "If statement with mixed body should be mutable"
        );
        assert!(
            mutatable_lines.contains(&3),
            "Non-arid line in body should be mutable"
        );
    }
}
