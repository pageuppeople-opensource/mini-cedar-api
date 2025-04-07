# A super-simple rest api to expose the cedar policy engine

Intended to be used as part of mocking AWS Amazon Verified Permissions until such time that localstack supports this service. Have this api called from a fake `IAmazonVerifiedPermissions` implementation to handle your validation and authZ checks, and separately implement a policy store.

## Quick start 

Using rust:

```sh
cargo run
```

Using docker:

```sh
docker compose up
```

Or use the [repository image](https://github.com/pageuppeople-opensource/mini-cedar-api/pkgs/container/mini-cedar-api).

After running, the api is available at http://localhost:3000

## APIs

Check out the [rest api file](./rest-apis.http) for examples.

- `GET /` Health check endpoint, returns `"Hello from Rust!"`
- `POST /bulk-has-access` Run many access checks
  - Request:
    ```ts
    type request = {
        "checks": {
            "principal": string,
            "action": string,
            "resource": string,
            "context"?: {} // same as attrs below, see samples in cedar repos
        }[],
        "entities": {
            "uid": { "type": string, "id": string },
            "attrs": { }, // see sample entities in cedar repos
            "parents": { "type": string, "id": string }[]
        }[] | string, // alternatively just use cedar-json
        "policies": {
            "static_policies": { "id": string, "statement": string }[],
            "templated_policies": {
                "id": string,
                "template_id": string,
                "principal"?: string,
                "resource"?: string
            }[],
            "templates": { "id": string, "statement": string }[],
        },
        "schema": string
    }
    ```
  - Response:
    ```ts
    type response = {
        "decision": "Allow" | "Deny",
        "diagnostics": {
            "reason": string[],
            "errors": string[]
        }
    }[]
    ```
- `POST /validate/schema` Parse and validate a schema
  - Request:
    ```ts
    type request = {
        "schema": string
    }
    ```
  - Response (200 if ok, 400 otherwise):
    ```ts
    type response = string[] // errors, if any
    ```
- `POST /validate/template` Parse and validate a template against a schema
  - Request:
    ```ts
    type request = {
        "schema": string,
        "template_statement": string
    }
    ```
  - Response (200 if ok, 400 otherwise):
    ```ts
    type response = string[] // errors, if any
    ```
- `POST /validate/static-policy` Parse and validate a static policy against a schema
  - Request:
    ```ts
    type request = {
        "schema": string,
        "policy_statement": string
    }
    ```
  - Response (200 if ok, 400 otherwise):
    ```ts
    type response = string[] // errors, if any
    ```
- `POST /validate/templated_policy` Parse and validate a templated policy against a schema and template
  - Request:
    ```ts
    type request = {
        "schema": string,
        "template_statement": string,
        "principal"?: string,
        "resource"?: string
    }
    ```
  - Response (200 if ok, 400 otherwise):
    ```ts
    type response = string[] // errors, if any
    ```

## Maintaining

This code is by no means an example of "good" or "idiomatic" Rust. In particular, the error handling is done very poorly, but for now "it works".

As AVP improves support for cedar, we will need to update the cedar version used.
This can be done by modifying the [cargo.toml](./Cargo.toml) file to update `cedar-policy` to the newly supported version. Then run `cargo update` and `cargo run`.
