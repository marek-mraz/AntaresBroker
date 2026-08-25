*** Settings ***
Documentation       Check that notification pick applies the full Attribute Projection
...                 Language of CIM 009 clause 4.21, as Table 5.2.14.1-1 requires
...                 ("a valid attribute projection language string as per clause 4.21"):
...                 a ProjectionTerm may carry one LinkedEntityTerm
...                 (AttrName *1(LinkedEntityTerm)) and or-separated alternatives
...                 (orOp = "|" / ","). A term like locatedAt{name} must keep the
...                 locatedAt Relationship in the notified entity — not degrade to a
...                 literal attribute name matching nothing — and a "name|street"
...                 element must project both alternatives. Official TPs 046_42/046_43
...                 cover only flat single-name pick/omit.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/NotificationUtils.resource
Resource            ${EXECDIR}/resources/SubscriptionUtils.resource

Test Teardown       Delete Test Subscription And Entity


*** Variables ***
${building_filename}=       building-different-attributes-types.jsonld


*** Test Cases ***
046_44_01 Linked Entity Term In Pick Keeps The Relationship In The Notification
    [Documentation]    pick=["id","name","locatedAt{name}"]: the LinkedEntityTerm
    ...    constrains Attributes within the Linked Entity, so the base
    ...    Relationship locatedAt stays in the payload; street and location
    ...    must not appear
    [Tags]    sub-notification    4_21    5_2_14    5_8_6    since_v1.9.1
    [Setup]    Setup Subscription And Entity    subscriptions/subscription-building-entities-pick-linked-term.jsonld

    ${response}=    Update Entity Attributes    ${entity_id}    name-fragment.jsonld    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    pick-omit/entity-pick-linked-term.json
    ...    ${notification}

046_44_02 Pipe Disjunction In One Pick Element Projects Both Alternatives
    [Documentation]    pick=["id","name|street"]: the pipe is the 4.21 or-operator,
    ...    so both name and street are projected; locatedAt and location
    ...    must not appear
    [Tags]    sub-notification    4_21    5_2_14    5_8_6    since_v1.9.1
    [Setup]    Setup Subscription And Entity    subscriptions/subscription-building-entities-pick-disjunction.jsonld

    ${response}=    Update Entity Attributes    ${entity_id}    name-fragment.jsonld    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    pick-omit/entity-pick-disjunction.json
    ...    ${notification}


046_44_03 Linked Entity Term Projects Inside The Joined Linked Entity
    [Documentation]    join=flat with pick=["id","name","locatedAt{id|name}"]: the
    ...    LinkedEntityTerm applies to the Linked Entity reached through
    ...    locatedAt, reducing the joined City to id and name; its
    ...    description and isInCountry must not appear, and the pipe
    ...    inside the braces is the 4.21 or-operator
    [Tags]    sub-notification    4_21    4_5_23    5_2_14    5_8_6    since_v1.9.1
    [Setup]    Setup Subscription And Linked Entities    subscriptions/subscription-building-entities-join-flat-pick-linked-term.jsonld
    [Teardown]    Delete Test Subscription And Linked Entities

    ${response}=    Update Entity Attributes    ${linking_entity_id}    name-fragment.jsonld    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}

    ${notification}    ${headers}=    Wait for notification    timeout=${10}

    Should be Equal    ${subscription_id}    ${notification}[subscriptionId]
    Check Notification Containing Entities Elements
    ...    pick-omit/entity-join-flat-pick-linked-term.json
    ...    ${notification}


*** Keywords ***
Setup Subscription And Linked Entities
    [Arguments]    ${subscription_payload_file_path}
    Create Subscription And Entity With Linked Entity
    ...    ${subscription_payload_file_path}
    ...    building-relationship.jsonld
    ...    046_44:EiffelTower
    ...    city-simple-attributes.jsonld
    ...    Paris
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Test Subscription And Linked Entities
    Delete Subscription    ${subscription_id}
    Delete Entity    ${linking_entity_id}
    Delete Entity    ${linked_entity_id}
    Stop Local Server

Setup Subscription And Entity
    [Arguments]    ${subscription_payload_file_path}
    Create Subscription And Entity    ${subscription_payload_file_path}    ${building_filename}    046_44
    Start Local Server    ${notification_server_host}    ${notification_server_port}

Delete Test Subscription And Entity
    Delete Subscription    ${subscription_id}
    Delete Entity    ${entity_id}
    Stop Local Server
