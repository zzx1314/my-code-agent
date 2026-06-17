with open('main.rs', 'r') as f:
    content = f.read()

# Fix 1: Site 1 - add missing closing brace before CommandResult::ResponseSent
old1 = """                                    current_session_name = new_session_name;
                                CommandResult::ResponseSent => {}"""

new1 = """                                    current_session_name = new_session_name;
                                }
                                CommandResult::ResponseSent => {}"""

content = content.replace(old1, new1, 1)

# Fix 2: Site 2 - fix the orphaned } => { block
old2 = """                                    current_session_name = new_session_name;
                                }
                                } => {

                        None => {"""

new2 = """                                    current_session_name = new_session_name;
                                }
                                CommandResult::ResponseSent => {}
                            }
                        }
                        None => {"""

content = content.replace(old2, new2, 1)

# Fix 3: Remove the 3 extra closing braces
old3 = "    }\n    }\n    }\n    tracing::info!(\"Headless mode shut down\");"
new3 = "    tracing::info!(\"Headless mode shut down\");"
content = content.replace(old3, new3, 1)

with open('main.rs', 'w') as f:
    f.write(content)

print("All fixes applied")
print(f"old1 found: {old1 in content}")
print(f"old2 found: {old2 in content}")
print(f"old3 found: {old3 in content}")
