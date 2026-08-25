*** Settings ***
Documentation       Check that one gets an error when try to delete a @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial @context
Test Teardown       Delete Initial @context
Test Template       Delete a @context with wrong data


*** Variables ***
${filename}=        @context-minimal-valid.json
${cached_id}=       https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld
${reason_400}=      Bad Request
${reason_404}=      Not Found
${reason_422}=      Unprocessable


*** Test Cases ***    CONTEXTID    RELOAD    STATUSCODE    REASON    ERROR
051_04_01 Delete A @contexts With A Wrong Id And Reload Set To True
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    wrong_id_context    true    400    ${reason_400}    ${ERROR_TYPE_BAD_REQUEST_DATA}
051_04_02 Delete A @contexts With A Wrong Id And Reload Set To False
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    wrong_id_context    false    404    ${reason_404}    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
051_04_03 Delete A @contexts With A Wrong Id And No Reload Value
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    wrong_id_context    ${EMPTY}    404    ${reason_404}    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
051_04_04 Delete A @contexts With A Wrong Id And Wrong Reload Value
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    wrong_id_context    xxx    400    ${reason_400}    ${ERROR_TYPE_BAD_REQUEST_DATA}
051_04_05 Delete A Hosted @contexts With A Valid Id And Reload Set To True
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    ${uri}    true    400    ${reason_400}    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Delete a @context with wrong data
    [Documentation]    Check that one can delete a hosted @context
    [Arguments]    ${contextid}    ${reload}    ${statuscode}    ${reason}    ${error}
    ${response}=    Delete a @context    ${contextid}    ${reload}

    Check Response Status Code    ${statuscode}    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason}
    Check Response Body Containing ProblemDetails Element    ${response.json()}    ${error}

Create Initial @context
    ${response}=    Add a new @context    ${filename}
    Check Response Status Code    201    ${response.status_code}
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${uri}

Delete Initial @context
    Delete a @context    ${uri}
