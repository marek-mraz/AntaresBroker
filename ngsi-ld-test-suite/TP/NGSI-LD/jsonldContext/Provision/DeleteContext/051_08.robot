*** Settings ***
Documentation       Check that one gets an error when try to delete the core @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Template       Delete a core @context


*** Variables ***
${reason_400}=      Bad Request
${type}=            https://uri.etsi.org/ngsi-ld/errors/BadRequestData


*** Test Cases ***    RELOAD
051_08_01 Delete A Core @contexts And Return An Error With No Reload
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    ${EMPTY}
051_08_02 Delete A Core @contexts And Return An Error With Reload Set To False
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    false


*** Keywords ***
Delete a core @context
    [Documentation]    Check that one gets an error when try to delete the core @context
    [Arguments]    ${reload}

    ${response}=    Delete a @context    ${core_context}    ${reload}

    Check Response Status Code    400    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_400}
    Check Response Body Containing ProblemDetails Element    ${response.json()}    ${type}
