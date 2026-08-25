*** Settings ***
Documentation       Check that one gets an error if one created an entity with a context (Cached context) and one tries to delete it with reload=true

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
051_05_01 Delete And Reload A Cached @context With No Communication With The Context Server
    [Documentation]    Check that one gets an error if one tries to reload a cached context with no communication with the context server
    [Tags]    ctx-serve    5_13_5    since_v1.5.1

    ${response}=    Delete a @context    ${uri}    true

    Check Response Status Code    504    ${response.status_code}
    Check Response Reason set to    ${response.reason}    Gateway Timeout
    Check Response Body Containing ProblemDetails Element
    ...    ${response.json()}
    ...    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}


*** Keywords ***
Create Initial @context condition from an external server
    Start @context Local Server

    # The @context URL must be absolute so the broker can fetch+cache it from the local mock server.
    # Original bug 1: the Catenate was commented out, leaving ${uri} relative (/api/v1/context.jsonld)
    # -> unresolvable -> never Cached. Bug 2: a prior mock test does "Set Global Variable ${uri}" with
    # the absolute URL, which leaks here and would make the Catenate double the host. Reset to the
    # relative path first, then build the absolute URL (mirrors 053_05).
    ${uri}=    Set Variable    /api/v1/context.jsonld
    ${uri}=    Catenate    SEPARATOR=    http://${context_server_host}:${context_server_port}    ${uri}
    Set Global Variable    ${uri}

    Create Entity selecting @context    ${entityfile}    ${uri}

    Log    Waiting 3 seconds to continue...
    Sleep    3s
    ${response}=    List @contexts
    ${response}=    Serve a @context    ${uri}    true
    Check Response Status Code    200    ${response.status_code}
    Check Context Response Kind    ${response.json()}    Cached

    Stop @context Local Server

Delete Initial @context condition from an external server
    Log    Delete initial contidions
    Delete Entity    ${entity_context_id}
    Delete a @context    ${uri}
