*** Settings ***
Documentation       Check that the context served by a context server is still in the broker after a ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE with details=true

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
${reason_200}=              OK


*** Test Cases ***
053_05_01 Check That The Context Served By A Context Server Is Still In The Broker After A ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE With Details=true
    [Documentation]    Check that the context served by a context server is still in the broker after a ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE with details=true
    [Tags]    ctx-serve    5_13_4    since_v1.5.1
    ${response}=    List @contexts    ${TRUE}
    Log    ${response}
    ${response}=    Serve a @context
    ...    contextId=${uri}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}
    Check Response Headers Containing Content-Type set to    ${CONTENT_TYPE_JSON}    ${response.headers}

    # Check mandatory keys in the response (URL, localId, kind, timestamp) and their possible values
    Check Context Response Body Containing Detailed Information    ${response.json()}    Cached

    # Check optional keys in the response (lastUsage, numberOfHits, extraInfo) and their possible values
    Check Response Body Might Contain Optional Fields    ${response.json()}    lastUsage
    Check Response Body Might Contain Optional Fields    ${response.json()}    numberOfHits
    Check Response Body Might Contain Optional Fields    ${response.json()}    extraInfo

    # Check that there is no other keys
    Check Context Detailed Information Keys    ${response.json()}


*** Keywords ***
Create Initial @context condition from an external server
    Start @context Local Server

    ${uri}=    Catenate    http://${context_server_host}:${context_server_port}${uri}
    Set Global Variable    ${uri}

    Create Entity selecting @context    ${entityfile}    ${uri}

    Log    Waiting 3 seconds to continue...
    Sleep    3s
    ${response}=    List @contexts    ${TRUE}
    Log    ${response}
    ${response}=    Serve a @context    ${uri}    true
    Check Response Status Code    200    ${response.status_code}
    Check Context Response Kind    ${response.json()}    Cached

    Stop @context Local Server

    ${response}=    Delete a @context    ${uri}    true
    Check Response Status Code    504    ${response.status_code}

Delete Initial @context condition from an external server
    Log    Delete initial contidions
    Delete Entity    ${entity_context_id}
    Delete a @context    ${uri}
