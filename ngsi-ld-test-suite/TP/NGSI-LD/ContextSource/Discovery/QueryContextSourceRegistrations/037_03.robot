*** Settings ***
Documentation       Check that one cannot query context source registrations, if the list of Entity identifiers includes a URI which it is not valid, or the query, geo-query or temporal query are not syntactically valid

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Query Context Source Registration With Invalid Query Param


*** Test Cases ***    QUERY_PARAM_NAME    QUERY_PARAM_VALUE
037_03_01 Invalid URI
    [Tags]    csr-query    5_10_2
    id    invalidUri
037_03_02 Invalid Query
    [Tags]    csr-query    5_10_2
    q    (invalidQuery
037_03_03 Invalid GeoQuery
    [Tags]    csr-query    5_10_2
    georel    within
037_03_04 Invalid Temporal Query
    [Tags]    csr-query    5_10_2
    timerel    before


*** Keywords ***
Query Context Source Registration With Invalid Query Param
    [Documentation]    Check that one cannot query context source registrations, if the list of Entity identifiers includes a URI which it is not valid, or the query, geo-query or temporal query are not syntactically valid
    [Arguments]    ${query_param_name}    ${query_param_value}
    ${response}=    Query Context Source Registrations
    ...    context=${ngsild_test_suite_context}
    ...    ${query_param_name}=${query_param_value}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
