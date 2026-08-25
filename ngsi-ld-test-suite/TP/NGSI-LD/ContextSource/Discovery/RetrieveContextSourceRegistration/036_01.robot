*** Settings ***
Documentation       Check that one cannot retrieve a Context Source Registration, if the context source registration id is not a valid URI

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Retrieve Context Source Registration With An Invalid Id


*** Test Cases ***    ID
036_01_01 Invalid Id
    [Tags]    csr-retrieve    5_10_1
    invalidUri


*** Keywords ***
Retrieve Context Source Registration With An Invalid Id
    [Documentation]    Check that one cannot retrieve a Context Source Registration, if the context source registration id is not a valid URI
    [Arguments]    ${id}
    ${response}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${id}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
