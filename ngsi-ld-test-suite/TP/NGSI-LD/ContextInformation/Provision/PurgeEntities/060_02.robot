*** Settings ***
Documentation       Check that a BadRequestData error is returned when purge entities is called with both keep and drop parameters

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
060_02_01 Purge Entities With Keep And Drop At The Same Time
    [Documentation]    Check that a BadRequestData error is returned when both keep and drop query parameters are provided
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities
    ...    type=Building
    ...    keep=name
    ...    drop=subCategory
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
