*** Settings ***
Documentation       If the count parameter is set to true the special HTTP header NGSILD-Results-Count is set in the response and it shall contain the total number of matching results.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Query Entities With Count


*** Variables ***
${first_entity_filename}=       building-simple-attributes.jsonld
${second_entity_filename}=      building-simple-attributes-second.jsonld
${entity_type}=                 Building
${airQualityLevel}=             airQualityLevel==6


*** Test Cases ***    Q_PARAMETER    EXPECTED_STATUS    EXPECTED_COUNT
019_07_01 Check That The Total Number Of Matching Results Is Returned If The Count Parameter Is Set To True And Only The Entity Type Is Provided
    [Tags]    e-query    5_7_2    6_3_13
    ${EMPTY}    200    2
019_07_02 Check That The Total Number Of Matching Results Is Returned If The Count Parameter Is Set To True And A Q Parameter Is Provided
    [Tags]    e-query    5_7_2    6_3_13
    ${airQualityLevel}    200    1


*** Keywords ***
Query Entities With Count
    [Documentation]    If the count parameter is set to true the special HTTP header NGSILD-Results-Count is set in the response and it shall contain the total number of matching results.
    [Arguments]    ${q}    ${expected_status_code}    ${expected_count}
    ${response}=    Query Entities
    ...    entity_types=${entity_type}
    ...    q=${q}
    ...    count=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Headers Containing NGSILD-Results-Count Equals To    ${expected_count}    ${response.headers}

Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${first_entity_filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${second_entity_filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Initial Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
