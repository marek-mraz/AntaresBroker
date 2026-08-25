*** Settings ***
Documentation       Check that one can delete a temporal representation of an entity with simple temporal properties

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Temporal Entity


*** Variables ***
${filename}=    vehicle-temporal-representation.jsonld


*** Test Cases ***
009_01_01 Delete A Temporal Representation Of An Entity With Simple Temporal Properties
    [Documentation]    Check that one can delete a temporal representation of an entity with simple temporal properties
    [Tags]    te-delete    5_6_16
    ${response}=    Delete Temporal Representation Of Entity With Returning Response
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    context=${ngsild_test_suite_context}
    Check SUT Not Containing Resource    ${response1.status_code}


*** Keywords ***
Create Initial Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${temporal_entity_representation_id}
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
