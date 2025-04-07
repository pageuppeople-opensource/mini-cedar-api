use std::{any::Any, collections::HashMap, str::FromStr};

use axum::{http::StatusCode, routing::{get, post}, Json, Router};
use bytes::Bytes;
use cedar_policy::{Authorizer, Context, Decision, Entities, EntityUid, Policy, PolicyId, PolicySet, Request, Response, Schema, SlotId, Template, ValidationMode, Validator};
use http::header;
use http_body_util::Full;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::catch_panic::CatchPanicLayer;

#[tokio::main]
async fn main() {
    // build our application with routes
    let app = Router::new()
        .route("/", get(health_check))
        .route("/bulk-has-access", post(run_bulk_access_check))
        .route("/validate/schema", post(validate_schema))
        .route("/validate/static-policy", post(validate_static_policy))
        .route("/validate/templated-policy", post(validate_templated_policy))
        .route("/validate/template", post(validate_template))
        // quick and dirty catch all panics and return as 500 with message
        .layer(CatchPanicLayer::custom(handle_panic));

    // run it
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("listening on :3000");
    axum::serve(listener, app).await.unwrap();
}

fn handle_panic(err: Box<dyn Any + Send + 'static>) -> http::Response<Full<Bytes>> {
    let details = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "Unknown panic message".to_string()
    };

    let body = serde_json::json!({
        "error": {
            "kind": "panic",
            "details": details,
        }
    });
    let body = serde_json::to_string(&body).unwrap();

    http::Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::from(body))
        .unwrap()
}

async fn health_check() -> Json<&'static str> {
    Json("Hello from Rust!")
}

#[axum::debug_handler]
async fn run_bulk_access_check(
    Json(payload): Json<BulkAccessCheckRequest>
) -> (StatusCode, Json<Vec<AccessCheckResponse>>) {
    println!("Payload: {:#?}", payload);

    // load static policies    
    let policies = parse_policy_set(payload.policies);

    // parse schema
    let schema = Schema::from_json_str(&payload.schema).unwrap();

    // parse entities
    let entities = if payload.entities.is_string() { 
        Entities::from_json_str(payload.entities.as_str().unwrap(), Some(&schema)).unwrap() 
    } else { 
        Entities::from_json_value(payload.entities, Some(&schema)).unwrap() 
    };

    let authorizer = Authorizer::new();

    let results = payload
        .checks
        .into_iter()
        .map(|check| run_access_check_internal(&authorizer, check, &policies, &entities, &schema))
        .map(|response| AccessCheckResponse {
            decision: response.decision(),
            diagnostics: Diagnostics {
                reason: response
                    .diagnostics()
                    .reason()
                    .map(|r| r.to_string())
                    .collect(),
                errors: response
                    .diagnostics()
                    .errors()
                    .map(|err| err.to_string())
                    .collect(),
            },
        })
        .collect();

    println!("Response: {:#?}", results);
    return (StatusCode::OK, Json(results));
}

async fn validate_schema(
    Json(payload): Json<ValidateSchemaRequest>
) -> (StatusCode, Json<Vec<String>>) {
    println!("Payload: {:#?}", payload);

    validate_policy_set_with_schema(&payload.schema, PolicySet::new())
}

async fn validate_static_policy(
    Json(payload): Json<ValidateStaticPolicyRequest>
) -> (StatusCode, Json<Vec<String>>) {
    println!("Payload: {:#?}", payload);

    let policy = match Policy::parse(Some(PolicyId::new("thePolicyId")), payload.policy_statement) {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(vec![e.to_string()]))
    };

    let mut policy_set = PolicySet::new();
    match policy_set.add(policy) {
        Ok(_) => (),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(vec![e.to_string()]))
    };

    validate_policy_set_with_schema(&payload.schema, policy_set)
}

async fn validate_template(
    Json(payload): Json<ValidateTemplateRequest>
) -> (StatusCode, Json<Vec<String>>) {
    println!("Payload: {:#?}", payload);

    let mut policy_set = PolicySet::new();

    match add_template(&mut policy_set, &"theTemplateId".into(), payload.template_statement) {
        Ok(_) => (),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(e))
    };

    validate_policy_set_with_schema(&payload.schema, policy_set)
}

async fn validate_templated_policy(
    Json(payload): Json<ValidateTemplatedPolicyRequest>
) -> (StatusCode, Json<Vec<String>>) {
    println!("Payload: {:#?}", payload);
    
    let template_id = "theTemplateId".to_owned();
    let mut policy_set = PolicySet::new();
    match add_template(&mut policy_set, &template_id, payload.template_statement) {
        Ok(_) => (),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(e))
    };

    match add_templated_policy(&mut policy_set, "thePolicyId".to_owned(), &template_id, payload.principal, payload.resource) {
        Ok(_) => (),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(e))
    };

    validate_policy_set_with_schema(&payload.schema, policy_set)
}

fn validate_policy_set_with_schema(schema: &str, policy_set: PolicySet) -> (StatusCode, Json<Vec<String>>) {
    let schema = match Schema::from_json_str(schema) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(vec![e.to_string()]))
    };

    let validator = Validator::new(schema);
    let result = validator.validate(&policy_set, ValidationMode::Strict);

    if result.validation_passed() {
        return (StatusCode::OK, Json(Vec::new()))
    }

    return (StatusCode::BAD_REQUEST, Json(result.validation_errors().into_iter().map(|e| e.to_string()).collect()));
}

fn parse_policy_set(payload: InputAllPolicies) -> PolicySet {
    let mut policies = PolicySet::new();

    for static_policy in payload.static_policies {
        let policy = Policy::parse(Some(PolicyId::new(static_policy.id)), static_policy.statement).unwrap();
        policies.add(policy).unwrap();
    }

    // load templates
    for template in payload.templates  {
        add_template(&mut policies, &template.id, template.statement).unwrap();
    }

    // load templated policies
    for templated_policy in payload.templated_policies {
        add_templated_policy(&mut policies, templated_policy.id, &templated_policy.template_id, templated_policy.principal, templated_policy.resource).unwrap();
    }
    policies
}

fn add_templated_policy(policy_set: &mut PolicySet, policy_id: String, template_id: &String, principal: Option<String>, resource: Option<String>) -> Result<(), Vec<String>> {
    let mut slot_values = HashMap::new();
        
    if principal.is_some() {
        slot_values.insert(SlotId::principal(), EntityUid::from_str(&principal.unwrap()).unwrap());
    }

    if resource.is_some() {
        slot_values.insert(SlotId::resource(), EntityUid::from_str(&resource.unwrap()).unwrap());
    }
    
    policy_set.link(
        template_id.parse().unwrap(), 
        policy_id.parse().unwrap(), 
        slot_values)
        .or_else(|e| Err(vec![e.to_string()]))
}

fn add_template(policy_set: &mut PolicySet, template_id: &String, statement: String) -> Result<(), Vec<String>> {
    let template = match Template::parse(Some(PolicyId::new(template_id)), statement) {
        Ok(p) => p,
        Err(e) => return Err(vec![e.to_string()])
    };

    policy_set.add_template(template).or_else(|e| Err(vec![e.to_string()]))
}

fn run_access_check_internal(authorizer: &Authorizer, check: AccessCheck, policies: &PolicySet, entities: &Entities, _schema: &Schema) -> Response {
    // build request
    let action = check.action.parse().unwrap();
    let principal = check.principal.parse().unwrap();
    let resource = check.resource.parse().unwrap();
    let context = match check.context {
        Some(ctx) => match ctx.as_str() {
            Some(str) => Context::from_json_str(str, Some((_schema, &action))).unwrap(),
            None => Context::from_json_value(ctx, None).unwrap()
        },
        None => Context::empty(),
    };

    // TODO: Future versions of cedar support adding the schema to the request, do this when we update support
    let request = Request::new(principal, action, resource, context, Some(_schema)).unwrap();

    // run the check
    authorizer.is_authorized(&request, &policies, &entities)
}

#[derive(Deserialize, Debug)]
struct BulkAccessCheckRequest {
    checks: Vec<AccessCheck>,
    entities: Value,
    policies: InputAllPolicies,
    schema: String
}

#[derive(Deserialize, Debug)]
struct ValidateSchemaRequest {
    schema: String
}

#[derive(Deserialize, Debug)]
struct ValidateStaticPolicyRequest {
    schema: String,
    policy_statement: String
}

#[derive(Deserialize, Debug)]
struct ValidateTemplatedPolicyRequest {
    schema: String,
    principal: Option<String>,
    resource: Option<String>,
    template_statement: String
}

#[derive(Deserialize, Debug)]
struct ValidateTemplateRequest {
    schema: String,
    template_statement: String
}

#[derive(Deserialize, Debug)]
struct AccessCheck {
    principal: String,
    action: String,
    resource: String,
    context: Option<Value>
}

#[derive(Deserialize, Debug)]
struct InputAllPolicies {
    static_policies: Vec<InputStaticPolicy>,
    templated_policies: Vec<InputTemplatedPolicy>,
    templates: Vec<InputTemplate>
}

#[derive(Deserialize, Debug)]
struct InputStaticPolicy {
    id: String,
    statement: String
}

#[derive(Deserialize, Debug)]
struct InputTemplatedPolicy {
    id: String,
    principal: Option<String>,
    resource: Option<String>,
    template_id: String
}

#[derive(Deserialize, Debug)]
struct InputTemplate {
    id: String,
    statement: String
}

#[derive(Serialize, Debug)]
struct AccessCheckResponse {
    /// Authorization decision
    decision: Decision,
    /// Diagnostics providing more information on how this decision was reached
    diagnostics: Diagnostics,
}

#[derive(Serialize, Debug)]
struct Diagnostics {
    /// `PolicyId`s of the policies that contributed to the decision.
    /// If no policies applied to the request, this set will be empty.
    reason: Vec<String>,
    /// Errors that occurred during authorization. The errors should be
    /// treated as unordered, since policies may be evaluated in any order.
    errors: Vec<String>,
}