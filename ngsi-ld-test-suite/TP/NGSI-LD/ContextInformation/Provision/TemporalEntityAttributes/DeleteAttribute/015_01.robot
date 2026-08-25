*** Settings ***
Documentation       Check that one can delete an attribute of a temporal representation of an entity with simple temporal properties

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Initialize Setup
Test Teardown       Delete Temporal Entity
Test Template       Delete Attribute From A Temporal Entity


*** Variables ***
${filename}=        vehicle-temporal-representation.jsonld
${status_code}=     204
${attribute_id}=    fuelLevel


*** Test Cases ***    DELETE_ALL    DATASET_ID    EXPECTATION_FILE
015_01_01 Delete An Attribute From A Temporal Representation Of An Entity Without deleteAll/datasetId
    false    ${EMPTY}    vehicle-temporal-representation-delete-fuelLevel.jsonld
015_01_02 Delete An Attribute From A Temporal Representation Of An Entity With datasetId
    false    urn:ngsi-ld:Vehicle:12345-fuel    vehicle-temporal-representation-delete-fuelLevel-datasetid.jsonld
015_01_03 Delete An Attribute From A Temporal Representation Of An Entity With deleteAll
    true    ${EMPTY}    vehicle-temporal-representation-deleteall-fuelLevel.jsonld


*** Keywords ***
Delete Attribute From A Temporal Entity
    [Documentation]    Check that one can delete an attribute of a temporal representation of an entity with simple temporal properties
    [Tags]    tea-delete    5_6_13
    [Arguments]    ${delete_all}    ${dataset_id}    ${expectation_filename}
    ${response}=    Delete Attribute From Temporal Entity
    ...    entityId=${temporal_entity_representation_id}
    ...    attributeId=${attribute_id}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    datasetId=${datasetid}
    ...    deleteAll=${deleteall}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${temporal_entity_expectation_payload}=    Load Test Sample
    ...    temporalEntities/expectations/${expectation_filename}
    ...    ${temporal_entity_representation_id}
    ${response1}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_attributes}=    Create List    instanceId    @context
    Check Updated Resource Set To
    ...    updated_resource=${temporal_entity_expectation_payload}
    ...    response_body=${response1.json()}
    ...    ignored_keys=${ignored_attributes}

Initialize Setup
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${temporal_entity_representation_id}
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
