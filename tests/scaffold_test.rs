use shae::scaffold::{ScaffoldOptions, Template, scaffold_project};
use std::fs;
use std::str::FromStr;

#[test]
fn test_template_from_str() {
    assert_eq!(Template::from_str("cli").unwrap(), Template::Cli);
    assert_eq!(Template::from_str("default").unwrap(), Template::Cli);
    assert_eq!(Template::from_str("game").unwrap(), Template::Game);
    assert_eq!(Template::from_str("arcade").unwrap(), Template::Game);
    assert_eq!(
        Template::from_str("actor-service").unwrap(),
        Template::ActorService
    );
    assert_eq!(
        Template::from_str("actors").unwrap(),
        Template::ActorService
    );
    assert_eq!(Template::from_str("web-app").unwrap(), Template::WebApp);
    assert_eq!(Template::from_str("http").unwrap(), Template::WebApp);
    assert!(Template::from_str("nonexistent_tpl").is_err());
}

#[test]
fn test_scaffold_cli_project() {
    let tmp_dir = std::env::temp_dir().join("shae_test_scaffold_cli");
    let _ = fs::remove_dir_all(&tmp_dir);

    let opts = ScaffoldOptions {
        name: "test_cli_tool".to_string(),
        template: Template::Cli,
        target_dir: Some(tmp_dir.clone()),
        force: false,
    };

    let result = scaffold_project(&opts);
    assert!(result.is_ok(), "Scaffolding cli failed: {:?}", result);

    assert!(tmp_dir.join("shae.toml").exists());
    assert!(tmp_dir.join(".gitignore").exists());
    assert!(tmp_dir.join("README.md").exists());
    assert!(tmp_dir.join("src/main.shae").exists());

    let manifest = fs::read_to_string(tmp_dir.join("shae.toml")).unwrap();
    assert!(manifest.contains("name = \"test_cli_tool\""));
    assert!(manifest.contains("entry = \"src/main.shae\""));

    let code = fs::read_to_string(tmp_dir.join("src/main.shae")).unwrap();
    assert!(code.contains("test_cli_tool"));
    assert!(code.contains("sys.args()"));

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_scaffold_game_project() {
    let tmp_dir = std::env::temp_dir().join("shae_test_scaffold_game");
    let _ = fs::remove_dir_all(&tmp_dir);

    let opts = ScaffoldOptions {
        name: "retro_blaster".to_string(),
        template: Template::Game,
        target_dir: Some(tmp_dir.clone()),
        force: false,
    };

    let result = scaffold_project(&opts);
    assert!(result.is_ok());

    let code = fs::read_to_string(tmp_dir.join("src/main.shae")).unwrap();
    assert!(code.contains("import \"std:gui\" as gui"));
    assert!(code.contains("gui.init_window"));
    assert!(code.contains("gui.render(win)"));

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_scaffold_actor_project() {
    let tmp_dir = std::env::temp_dir().join("shae_test_scaffold_actor");
    let _ = fs::remove_dir_all(&tmp_dir);

    let opts = ScaffoldOptions {
        name: "cluster_node".to_string(),
        template: Template::ActorService,
        target_dir: Some(tmp_dir.clone()),
        force: false,
    };

    let result = scaffold_project(&opts);
    assert!(result.is_ok());

    let code = fs::read_to_string(tmp_dir.join("src/main.shae")).unwrap();
    assert!(code.contains("import \"std:actor\" as actor"));
    assert!(code.contains("actor.spawn"));

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_scaffold_web_project() {
    let tmp_dir = std::env::temp_dir().join("shae_test_scaffold_web");
    let _ = fs::remove_dir_all(&tmp_dir);

    let opts = ScaffoldOptions {
        name: "api_gateway".to_string(),
        template: Template::WebApp,
        target_dir: Some(tmp_dir.clone()),
        force: false,
    };

    let result = scaffold_project(&opts);
    assert!(result.is_ok());

    let code = fs::read_to_string(tmp_dir.join("src/main.shae")).unwrap();
    assert!(code.contains("import \"std:json\" as json"));
    assert!(code.contains("/api/status"));

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_collision_detection_and_force() {
    let tmp_dir = std::env::temp_dir().join("shae_test_scaffold_collision");
    let _ = fs::remove_dir_all(&tmp_dir);
    fs::create_dir_all(&tmp_dir).unwrap();
    fs::write(tmp_dir.join("dummy.txt"), "hello").unwrap();

    let opts_no_force = ScaffoldOptions {
        name: "collision_pkg".to_string(),
        template: Template::Cli,
        target_dir: Some(tmp_dir.clone()),
        force: false,
    };

    let err = scaffold_project(&opts_no_force);
    assert!(
        err.is_err(),
        "Should error on non-empty directory without force"
    );

    let opts_force = ScaffoldOptions {
        name: "collision_pkg".to_string(),
        template: Template::Cli,
        target_dir: Some(tmp_dir.clone()),
        force: true,
    };

    let ok = scaffold_project(&opts_force);
    assert!(ok.is_ok(), "Should succeed with force=true");

    let _ = fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_invalid_name_rejected() {
    let opts = ScaffoldOptions {
        name: "../evil_traversal".to_string(),
        template: Template::Cli,
        target_dir: None,
        force: false,
    };

    let err = scaffold_project(&opts);
    assert!(err.is_err(), "Must reject path traversal in package name");
}
