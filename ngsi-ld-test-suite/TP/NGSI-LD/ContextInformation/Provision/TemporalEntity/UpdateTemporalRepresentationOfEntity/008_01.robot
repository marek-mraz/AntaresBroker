*** Settings ***
Documentation       Check that one can update a temporal representation of an entity with simple temporal properties

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Temporal Entity
Suite Teardown      Delete Temporal Entity


*** Variables ***
${filename}=                vehicle-create-temporal-representation.jsonld
${update_filename}=         vehicle-temporal-representation-update.jsonld
${expectation_filename}=    vehicle-temporal-representation-update.jsonld


*** Test Cases ***
008_01_01 Update A Temporal Representation Of An Entity With Simple Temporal Properties
    [Documentation]    Check that one can update a temporal representation of an entity with simple temporal properties
    [Tags]    te-update    5_6_11
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${update_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${temporal_entity_expectation_payload}=    Load Test Sample
    ...    temporalEntities/expectations/${expectation_filename}
    ...    ${temporal_entity_representation_id}
    ${response2}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_attributes}=    Create List    instanceId    @context
    Check Updated Resource Set To
    ...    updated_resource=${temporal_entity_expectation_payload}
    ...    response_body=${response2.json()}
    ...    ignored_keys=${ignored_attributes}


*** Keywords ***
Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${temporal_entity_representation_id}
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
