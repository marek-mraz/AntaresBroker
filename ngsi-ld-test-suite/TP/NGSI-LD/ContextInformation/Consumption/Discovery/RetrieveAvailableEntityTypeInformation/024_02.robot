*** Settings ***
Documentation       Check that one can retrieve a detailed representation of a specified NGSI-LD entity type

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Retrieve Detailed Representation Of Available Entity Type


*** Variables ***
${filename}=            building-simple-attributes.json
${expectation_file}=    types/expectations/entity-type-info-024-01.json


*** Test Cases ***    TYPE
024_02_01 WithCompactedType
    [Tags]    ed-type    5_7_7
    Building
024_02_02 WithExpandedType
    [Tags]    ed-type    5_7_7
    https%3A%2F%2Fngsi-ld-test-suite%2Fcontext%23Building


*** Keywords ***
Retrieve Detailed Representation Of Available Entity Type
    [Documentation]    Check that one can retrieve a detailed representation of a specified NGSI-LD entity type
    [Arguments]    ${type}
    ${response}=    Retrieve Entity Type    type=${type}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTypeInfo element    ${expectation_file}    ${response.json()}

Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    ${second_entity_id}=    Generate Random Building Entity Id
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Suite Variable    ${first_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Suite Variable    ${second_entity_id}

Delete Initial Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
