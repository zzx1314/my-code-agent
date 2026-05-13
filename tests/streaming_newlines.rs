use my_code_agent::ui::render::{render_full, render_streaming_markdown};

/// 验证流式 markdown 渲染对前导 \n\n 的处理
#[test]
fn test_streaming_with_leading_newlines() {
    // 模拟工具调用后的场景：streaming_text = "文本1\n\n文本2"
    let text = "I'll check the file.\n\nBased on the file, the answer is 42.\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
    
    let streaming_lines = render_streaming_markdown(text, None);
    
    // 验证换行没有被吃掉
    let non_empty_count = streaming_lines.iter()
        .filter(|l| {
            let s = format!("{:?}", l);
            !s.trim().is_empty() && s != "\"\""
        })
        .count();
    
    println!("streaming_text:\n{}", text);
    println!("total lines: {}", streaming_lines.len());
    println!("non-empty lines: {}", non_empty_count);
    for (i, line) in streaming_lines.iter().enumerate() {
        println!("  [{}] {:?}", i, line);
    }
    
    // 非空行应该远多于2（至少含 I'll, Based on, ```rust, fn main, ..., ```）
    assert!(non_empty_count >= 5, "Expected at least 5 non-empty lines, got {}", non_empty_count);
}

/// 验证 streaming_text 包含前导 \n\n 时的行为
#[test]
fn test_streaming_with_only_leading_newlines() {
    // 模拟模型直接调工具（无文本输出），然后文本 delta 到达
    // process_streaming_events: streaming_text = "\n\n" + "Based on the file..."
    let text = "\n\nBased on the file, here is the code:\n\n```rust\nfn main() {}\n```";
    
    let lines = render_streaming_markdown(text, None);
    
    println!("text with leading \\n\\n:\n{:?}", text);
    println!("rendered lines ({} total):", lines.len());
    for (i, line) in lines.iter().enumerate() {
        println!("  [{}] {:?}", i, line);
    }
    
    // 前导 \n\n 应该被正确处理，不导致换行丢失
    // 第一行是空行（来自第一个 \n），第二行也是空行（来自第二个 \n），第三行开始是内容
    assert!(lines.len() >= 3, "Expected at least 3 lines, got {}", lines.len());
    
    // 验证内容行存在
    let has_content = lines.iter().any(|l| {
        let s = format!("{:?}", l);
        s.contains("Based on the file")
    });
    assert!(has_content, "Content 'Based on the file' not found in rendered lines");
}

/// 验证 streaming 和 full 渲染的一致性
#[test]
fn test_streaming_full_consistency() {
    let text = "I'll check the file.\n\nBased on the file, the answer is 42.";
    let streaming = render_streaming_markdown(text, None);
    let full = render_full(text, None);
    
    assert_eq!(streaming.len(), full.len(), "streaming and full should produce same number of lines for same input");
}
