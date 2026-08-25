*** Settings ***
Documentation       Check that one cannot retrieve a detailed representation of an unknown NGSI-LD entity type

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities


*** Variables ***
${filename}=    building-simple-attributes.json


*** Test Cases ***
024_01_01 Retrieve Detailed Representation Of Available Entity Type Without Context
    [Documentation]    Check that one cannot retrieve a detailed representation of an unknown NGSI-LD entity type
    [Tags]    ed-type    5_7_7
    ${response}=    Retrieve Entity Type    type=Building
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
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
