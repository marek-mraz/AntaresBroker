*** Settings ***
Documentation       Verify 5.5.6 error handling for INVALID remote @contexts.
...
...                 5.5.6: "When a remote JSON-LD @context referenced by an
...                 incoming request is not available, implementations shall
...                 raise an error of type LdContextNotAvailable. If the
...                 remote JSON-LD @context is invalid, implementations shall
...                 raise an error of type BadRequestData."
...
...                 Antares extension TP — 043_01 covers the unavailable arm;
...                 this covers the retrieved-but-invalid arm (non-JSON body,
...                 JSON without an @context member) → 400 BadRequestData.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Library             HttpCtrl.Server


*** Test Cases ***
556_01_01 Remote Context Without @context Member Is BadRequestData
    [Documentation]    5.5.6: the remote document is retrieved (200) but is
    ...    not a JSON-LD @context (no @context member) → 400 BadRequestData,
    ...    NOT 504 LdContextNotAvailable.
    [Tags]    e-create    cb-ldcontext    5_5_6    since_v1.9.1
    Start Server    ${context_server_host}    ${context_server_port}
    Set Stub Reply    GET    /api/v1/invalid.jsonld    200    {"note": "no context member here"}
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${ctx_url}=    Set Variable    http://${context_server_host}:${context_server_port}/api/v1/invalid.jsonld
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": $ctx_url}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    [Teardown]    Stop Server

556_01_02 Remote Context That Is Not JSON Is BadRequestData
    [Documentation]    5.5.6: the remote document is retrieved (200) but is
    ...    not valid JSON → 400 BadRequestData; the entity must NOT exist.
    [Tags]    e-create    cb-ldcontext    5_5_6    since_v1.9.1
    Start Server    ${context_server_host}    ${context_server_port}
    Set Stub Reply    GET    /api/v1/broken.jsonld    200    this is { not json
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${ctx_url}=    Set Variable    http://${context_server_host}:${context_server_port}/api/v1/broken.jsonld
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": $ctx_url}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    404    ${response.status_code}
    [Teardown]    Stop Server
