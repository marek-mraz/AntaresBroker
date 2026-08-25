*** Settings ***
Documentation       Check that one cannot query context source registrations, if neither Entity types nor Attribute names are provided, an error of type 400 shall be raised.

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
037_02_01 Query Context Source Registrations Without Entity Types And Attribute Names
    [Documentation]    Check that one cannot query context source registrations, if neither Entity types nor Attribute names are provided, an error of type 400 shall be raised.
    [Tags]    csr-query    5_10_2
    ${response}=    Query Context Source Registrations    context=${ngsild_test_suite_context}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
