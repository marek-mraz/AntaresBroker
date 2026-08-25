*** Settings ***
Documentation       Verify 5.13.5.4: a failed reload leaves the stored @context in place.
...
...                 5.13.5.4: "If downloading fails, or the downloaded
...                 @context is invalid ... an error of type
...                 LdContextNotAvailable shall be raised ... In case of any
...                 error, the operation ends without removing the existing
...                 @context."
...
...                 Antares extension TP — 051_05 asserts only the 504 error;
...                 this asserts the Cached entry SURVIVES the failed reload
...                 and can still be deleted normally afterwards.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/ContextServerUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Library             Collections
Library             String
Variables           ${EXECDIR}/resources/variables.py
Library             HttpCtrl.Client
Library             HttpCtrl.Server

Test Setup          Create Initial @context condition from an external server
Test Teardown       Delete Initial @context condition from an external server


*** Variables ***
${filename}=                @context-minimal-valid.json
${entityfile}=              minimal-entity-using-@context.jsonld
${entity_context_id}=       urn:ngsi-ld:Testing:randomUUID
${uri}                      /api/v1/context.jsonld


*** Test Cases ***
5135_01_01 Failed Reload Keeps The Cached @context
    [Documentation]    5.13.5.4: the failed reload raises 504
    ...    LdContextNotAvailable AND the Cached entry stays in the storage —
    ...    its metadata is still served and a plain delete still succeeds.
    [Tags]    ctx-delete    5_13_5    since_v1.9.1

    ${response}=    Delete a @context    ${uri}    true
    Check Response Status Code    504    ${response.status_code}
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}

    # the operation ended WITHOUT removing the existing @context
    ${response}=    Serve a @context    ${uri}    true
    Check Response Status Code    200    ${response.status_code}
    Check Context Response Kind    ${response.json()}    Cached

    # a plain delete (no reload) still removes it
    ${response}=    Delete a @context    ${uri}
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Serve a @context    ${uri}    true
    Check Response Status Code    404    ${response.status_code}


*** Keywords ***
Create Initial @context condition from an external server
    Start @context Local Server

    # The @context URL must be absolute so the broker can fetch+cache it
    # from the local mock server (mirrors 051_05).
    ${uri}=    Set Variable    /api/v1/context.jsonld
    ${uri}=    Catenate    SEPARATOR=    http://${context_server_host}:${context_server_port}    ${uri}
    Set Global Variable    ${uri}

    Create Entity selecting @context    ${entityfile}    ${uri}

    Log    Waiting 3 seconds to continue...
    Sleep    3s
    ${response}=    Serve a @context    ${uri}    true
    Check Response Status Code    200    ${response.status_code}
    Check Context Response Kind    ${response.json()}    Cached

    Stop @context Local Server

Delete Initial @context condition from an external server
    Log    Delete initial conditions
    Delete Entity    ${entity_context_id}
    Delete a @context    ${uri}
