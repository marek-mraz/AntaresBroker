*** Settings ***
Documentation       Check that one gets an error when try to serve a @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial @context
Test Teardown       Delete Initial @context
Test Template       Serve @context with no previous created @context


*** Variables ***
${filename}=        @context-minimal-valid.json
${cached_id}=       https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld
${reason_400}=      Bad Request
${reason_404}=      Not Found
${reason_422}=      Unprocessable Content


*** Test Cases ***    CONTEXTID    DETAILS    STATUSCODE    REASON    ERROR
053_03_01 Serve A @contexts With A Wrong Id And Correct Details
    [Tags]    ctx-serve    5_13_4    since_v1.5.1
    wrong_id_context    true    404    ${reason_404}    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
# Deactivated because of boolean parsing
# 053_03_02 Serve a @contexts with a valid id and incorrect details
#    [Tags]    ctx-serve    5_13_4    since_v1.5.1
#    ${uri}    other    400    ${reason_400}    ${ERROR_TYPE_BAD_REQUEST_DATA}
# Why exactly should this produce a 422?
# 053_03_03 Serve a Cached @contexts with details set to false
#    [Tags]    ctx-serve    5_13_4    since_v1.5.1
#    ${cached_id}    false    422    ${reason_422}    ${ERROR_OPERATION_NOT_SUPPORTED}


*** Keywords ***
Serve @context with no previous created @context
    [Documentation]    Check that an error is returned when one requests for a @context that does not exist
    [Arguments]    ${contextid}    ${details}    ${statuscode}    ${reason}    ${error}
    ${response}=    Serve a @context    ${contextid}    ${details}

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
