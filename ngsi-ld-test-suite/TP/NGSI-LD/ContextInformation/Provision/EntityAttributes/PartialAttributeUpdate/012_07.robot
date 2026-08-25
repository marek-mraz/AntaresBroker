*** Settings ***
Documentation       Check that one can delete the default attribute instance using NGSI-LD Null in an Partial Attribute Update operation

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Update Attributes


*** Variables ***
${filename}=    building-different-default-attributes-instances-and-types.jsonld


*** Test Cases ***    STATUS_CODE    ATTR_NAME    FRAGMENT_FILENAME    EXPECTATION_FILENAME
012_07_01 Delete A Property
    [Tags]    ea-partial-update    5_6_4    6_7_3_1    since_v1.6.1
    204    name    ngsild-null/null-property-fragment.jsonld    ngsild-null/building-deleted-property-default-instance.jsonld
012_07_02 Delete A Relationship
    [Tags]    ea-partial-update    5_6_4    6_7_3_1    since_v1.6.1
    204    locatedAt    ngsild-null/null-relationship-fragment.jsonld    ngsild-null/building-deleted-relationship-default-instance.jsonld
012_07_03 Delete A GeoProperty
    [Tags]    ea-partial-update    5_6_4    6_7_3_1    since_v1.6.1
    204    location    ngsild-null/null-geoproperty-fragment.jsonld    ngsild-null/building-deleted-geoproperty-default-instance.jsonld
012_07_04 Delete A LanguageProperty
    [Tags]    ea-partial-update    5_6_4    6_7_3_1    4_5_18    since_v1.6.1
    204    street    ngsild-null/null-languageproperty-fragment.jsonld    ngsild-null/building-deleted-languageproperty-default-instance.jsonld


*** Keywords ***
Update Attributes
    [Documentation]    Check that one can delete the default attribute instance using NGSI-LD Null in an Partial Attribute Update operation
    [Arguments]
    ...    ${status_code}
    ...    ${attr_name}
    ...    ${fragment_filename}
    ...    ${expectation_filename}
    ${response}=    Partial Update Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attr_name}
    ...    fragment_filename=${fragment_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Body Containing Entity element
    ...    expectation_filename=${expectation_filename}
    ...    entity_id=${entity_id}
    ...    response_body=${response1.json()}

Delete Initial Entity
    Delete Entity    ${entity_id}

Setup Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
