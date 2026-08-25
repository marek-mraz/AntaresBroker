*** Settings ***
Documentation       Check that one can query all entities if query is local

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities


*** Variables ***
${filename}=                building-minimal.jsonld
${expectation_filename}=    building-minimal.json


*** Test Cases ***
019_25 Query All Entities With Local Flag
    [Documentation]    Check that one can query all entities if query is local
    [Tags]    e-query    5_7_2
    @{entities_ids_to_be_compared}=    Create List    ${first_entity_id}    ${second_entity_id}

    ${response}=    Query Entities
    ...    local=true

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Entity Elements
    ...    expectation_filename=${expectation_filename}
    ...    entities_ids=${entities_ids_to_be_compared}
    ...    response_body=${response.json()}


*** Keywords ***
Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
